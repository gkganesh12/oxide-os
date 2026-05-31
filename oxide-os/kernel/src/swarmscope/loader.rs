//! SwarmScope manifest loader.
//!
//! Takes a SwarmManifest and spawns the entire agent swarm on the Oxide OS
//! kernel — creating capabilities, IPC channels, mailboxes, and the
//! supervision tree.

use alloc::string::String;
use alloc::vec::Vec;
use x86_64::structures::paging::OffsetPageTable;

use super::manifest::*;
use super::broker;
use crate::agent::{self, AgentConfig, AgentId, ModelBinding, ResourceLimits, RestartPolicy};
use crate::capability::{CAP_TABLE, CapId, PermissionBits, ResourceRef};
use crate::ipc::{channel, message, MessageType};
use crate::println;
use crate::task::scheduler;

/// Result of spawning a swarm from a manifest.
pub struct SwarmSpawnResult {
    pub supervisor_id: AgentId,
    pub agent_ids: Vec<(String, AgentId)>,
    pub channel_count: usize,
    pub cap_count: usize,
}

/// Convert manifest permissions to kernel PermissionBits.
fn to_permission_bits(perms: &[Permission]) -> PermissionBits {
    let mut bits = PermissionBits::NONE;
    for p in perms {
        bits = bits.union(match p {
            Permission::Read => PermissionBits::READ,
            Permission::Write => PermissionBits::WRITE,
            Permission::Execute => PermissionBits::EXECUTE,
            Permission::Delegate => PermissionBits::DELEGATE,
            Permission::Spawn => PermissionBits::SPAWN,
            Permission::Kill => PermissionBits::KILL,
            Permission::Subscribe => PermissionBits::SUBSCRIBE,
            Permission::Publish => PermissionBits::PUBLISH,
            Permission::Connect => PermissionBits::CONNECT,
        });
    }
    bits
}

/// Convert manifest resource to kernel ResourceRef.
fn to_resource_ref(res: &ManifestResource) -> ResourceRef {
    match res {
        ManifestResource::Channel { name } => ResourceRef::Channel { name: name.clone() },
        ManifestResource::Network { host, port } => ResourceRef::Network {
            host: host.clone(),
            port: *port,
        },
        ManifestResource::Storage { path } => ResourceRef::Storage { path: path.clone() },
        ManifestResource::Tool { name } => ResourceRef::Tool { name: name.clone() },
        ManifestResource::Model { model_id } => ResourceRef::Model {
            model_id: model_id.clone(),
        },
        ManifestResource::Agent { agent_id } => ResourceRef::Agent(*agent_id),
        ManifestResource::AgentSpawn => ResourceRef::AgentSpawn,
    }
}

/// Convert manifest model to kernel ModelBinding.
fn to_model_binding(model: &ManifestModel) -> ModelBinding {
    match model {
        ManifestModel::Local { model_id } => ModelBinding::Local {
            model_id: model_id.clone(),
        },
        ManifestModel::Remote { endpoint } => ModelBinding::Remote {
            endpoint: endpoint.clone(),
            api_key_cap: 0,
        },
        ManifestModel::Auto { preference } => ModelBinding::Auto {
            preference: preference.clone(),
        },
    }
}

/// Convert manifest restart policy to kernel RestartPolicy.
fn to_restart_policy(policy: ManifestRestartPolicy) -> RestartPolicy {
    match policy {
        ManifestRestartPolicy::RestartOne => RestartPolicy::RestartOne,
        ManifestRestartPolicy::RestartAll => RestartPolicy::RestartAll,
        ManifestRestartPolicy::Escalate => RestartPolicy::Escalate,
        ManifestRestartPolicy::Permanent => RestartPolicy::Permanent,
    }
}

/// Auto-grant a network capability targeting the broker to the given agent.
/// Returns the CapId. Idempotent only in the sense that calling twice creates two caps.
fn auto_grant_broker_cap(agent_id: AgentId, endpoint: &str) -> Option<CapId> {
    let (host, port) = parse_endpoint(endpoint)?;
    let resource = ResourceRef::Network { host, port };
    let cap_id = CAP_TABLE
        .lock()
        .create_root(agent_id, resource, PermissionBits::CONNECT);
    Some(cap_id)
}

/// Parse "http://host:port" → ("host", port). Returns None on malformed input.
/// Ignores path component. Defaults port to 80 if absent.
fn parse_endpoint(endpoint: &str) -> Option<(String, u16)> {
    let stripped = endpoint
        .strip_prefix("http://")
        .or_else(|| endpoint.strip_prefix("https://"))
        .unwrap_or(endpoint);
    let host_port = stripped.split('/').next()?;
    let (host, port) = match host_port.find(':') {
        Some(i) => {
            let port: u16 = host_port[i + 1..].parse().ok()?;
            (String::from(&host_port[..i]), port)
        }
        None => (String::from(host_port), 80u16),
    };
    Some((host, port))
}

/// For each non-aggregator agent, create channel `agent.<name>.out`,
/// grant the agent PUBLISH on it, and grant the aggregator SUBSCRIBE on it.
///
/// Returns the per-agent publish CapId map so callers can store the publish cap
/// on the Agent record for later use by the entry function.
fn setup_agent_channels(
    manifest: &SwarmManifest,
    agent_ids: &[(String, AgentId)],
) -> alloc::collections::BTreeMap<String, CapId> {
    use alloc::collections::BTreeMap;

    let aggregator_id = agent_ids
        .iter()
        .find(|(name, _)| {
            manifest
                .agents
                .iter()
                .any(|a| a.name == *name && a.role == AgentRole::Aggregator)
        })
        .map(|(_, id)| *id);

    let mut publish_caps: BTreeMap<String, CapId> = BTreeMap::new();

    for (agent_name, agent_id) in agent_ids {
        let agent_def = match manifest.agents.iter().find(|a| a.name == *agent_name) {
            Some(d) => d,
            None => continue,
        };
        if agent_def.role == AgentRole::Aggregator {
            continue;
        }

        let channel_name = alloc::format!("agent.{}.out", agent_name);
        crate::ipc::channel::create(channel_name.clone());

        let mut table = CAP_TABLE.lock();
        let pub_cap = table.create_root(
            *agent_id,
            ResourceRef::Channel { name: channel_name.clone() },
            PermissionBits::PUBLISH,
        );
        publish_caps.insert(agent_name.clone(), pub_cap);

        if let Some(agg_id) = aggregator_id {
            table.create_root(
                agg_id,
                ResourceRef::Channel { name: channel_name },
                PermissionBits::SUBSCRIBE,
            );
        }
    }

    publish_caps
}

/// Generic agent entry point for SwarmScope simulation agents.
///
/// Each round: look up own name + caps + broker endpoint, POST to broker,
/// publish the parsed reply onto agent.<name>.out for the aggregator.
fn swarmscope_agent_entry() -> ! {
    use crate::interrupts;

    let my_id = scheduler::SCHEDULER.lock().current_id() as u64;

    // Brief stagger so agents don't all hit the broker at the same tick
    let stagger = (my_id as u64 % 8) * 50;
    let target = interrupts::ticks() + stagger;
    while interrupts::ticks() < target {
        if scheduler::should_reschedule() {
            scheduler::yield_now();
        }
    }

    // Snapshot what we need from the registry under one lock
    let (name, broker_cap, broker_endpoint, publish_cap) = {
        let reg = agent::registry::REGISTRY.lock();
        match reg.get(my_id) {
            Some(a) => (
                a.config.name.clone(),
                a.broker_cap,
                a.broker_endpoint.clone(),
                a.publish_cap,
            ),
            None => {
                println!("[swarm-agent:{}] not in registry — exiting", my_id);
                loop { scheduler::yield_now(); }
            }
        }
    };

    let broker_cap = match broker_cap {
        Some(c) => c,
        None => {
            println!("[swarm-agent:{}] no broker cap — idling", name);
            loop { scheduler::yield_now(); }
        }
    };

    let publish_channel = alloc::format!("agent.{}.out", name);
    println!(
        "[swarm-agent:{}] online — broker={} channel={}",
        name, broker_endpoint, publish_channel
    );

    let mut round = 0u32;
    let mut consecutive_failures = 0u32;
    let mut backoff_until: u64 = 0;

    loop {
        round += 1;

        if interrupts::ticks() < backoff_until {
            let target = interrupts::ticks() + 100;
            while interrupts::ticks() < target {
                if scheduler::should_reschedule() {
                    scheduler::yield_now();
                }
            }
            continue;
        }

        let prompt = alloc::format!("Round {}. What do you observe?", round);
        let start = interrupts::ticks();

        match broker::post_infer(&broker_endpoint, &name, &prompt, my_id, broker_cap) {
            Ok(reply) => {
                consecutive_failures = 0;
                let elapsed = interrupts::ticks().saturating_sub(start);
                let payload = alloc::format!(
                    "{{\"agent\":\"{}\",\"round\":{},\"text\":\"{}\",\"tokens_in\":{},\"tokens_out\":{},\"elapsed_ticks\":{}}}",
                    name,
                    round,
                    escape_for_log(&reply.text),
                    reply.tokens_in,
                    reply.tokens_out,
                    elapsed
                ).into_bytes();

                if let Some(pcap) = publish_cap {
                    if let Err(e) = crate::ipc::channel::publish(&publish_channel, my_id, payload, pcap) {
                        println!("[swarm-agent:{}] publish failed: {:?}", name, e);
                    }
                }

                if round % 5 == 0 {
                    println!(
                        "[swarm-agent:{}] round={} tokens_in={} tokens_out={} elapsed_ticks={}",
                        name, round, reply.tokens_in, reply.tokens_out, elapsed
                    );
                }
            }
            Err(e) => {
                println!("[swarm-agent:{}] infer failed: {:?}", name, e);
                consecutive_failures += 1;
                if consecutive_failures >= 3 {
                    backoff_until = interrupts::ticks() + 10000;
                    println!("[swarm-agent:{}] entering slow-poll (10s)", name);
                }
            }
        }

        // Sleep ~2s between rounds
        let target = interrupts::ticks() + 2000;
        while interrupts::ticks() < target {
            if scheduler::should_reschedule() {
                scheduler::yield_now();
            }
        }
    }
}

/// Replace control chars and quotes for safe embedding in a log payload.
fn escape_for_log(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push(' '),
            '\r' => {}
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Supervisor entry point — monitors the swarm.
fn swarmscope_supervisor_entry() -> ! {
    println!("[swarm-supervisor] Online — monitoring SwarmScope swarm");
    let mut report_num = 0u32;

    loop {
        let target = crate::interrupts::ticks() + 500;
        while crate::interrupts::ticks() < target {
            if scheduler::should_reschedule() {
                scheduler::yield_now();
            }
        }

        report_num += 1;
        let agent_count = agent::registry::REGISTRY.lock().count();
        let task_count = scheduler::SCHEDULER.lock().task_count();
        println!(
            "[swarm-supervisor] Report #{}: {} agents, {} tasks, tick={}",
            report_num,
            agent_count,
            task_count,
            crate::interrupts::ticks()
        );
    }
}

/// Aggregator entry point — collects findings from simulation agents.
fn swarmscope_aggregator_entry() -> ! {
    use crate::interrupts;

    let my_id = scheduler::SCHEDULER.lock().current_id() as u64;
    println!("[swarm-aggregator] Online — collecting simulation data");
    let mut total = 0u32;

    loop {
        while let Some(msg) = message::receive(my_id) {
            total += 1;
            if total % 20 == 0 {
                println!(
                    "[swarm-aggregator] {} messages collected (latest from agent {})",
                    total, msg.sender
                );
            }
        }

        if scheduler::should_reschedule() {
            scheduler::yield_now();
        }
    }
}

/// Reporter entry point — generates analysis.
fn swarmscope_reporter_entry() -> ! {
    use crate::interrupts;

    println!("[swarm-reporter] Online — will generate reports when data available");

    loop {
        let target = crate::interrupts::ticks() + 2000;
        while crate::interrupts::ticks() < target {
            if scheduler::should_reschedule() {
                scheduler::yield_now();
            }
        }
        println!("[swarm-reporter] Report cycle — awaiting aggregated data");
    }
}

/// Select the entry function based on agent role.
fn entry_for_role(role: AgentRole) -> fn() -> ! {
    match role {
        AgentRole::Supervisor => swarmscope_supervisor_entry,
        AgentRole::Aggregator => swarmscope_aggregator_entry,
        AgentRole::Reporter => swarmscope_reporter_entry,
        AgentRole::Individual | AgentRole::GroupRepresentative => swarmscope_agent_entry,
    }
}

/// Spawn an entire agent swarm from a SwarmManifest.
///
/// This is the main integration point. It:
/// 1. Creates all IPC channels
/// 2. Creates capability entries for each agent
/// 3. Spawns the supervisor, then all child agents under it
/// 4. Registers mailboxes for IPC
pub fn spawn_swarm(
    manifest: &SwarmManifest,
    mapper: &mut OffsetPageTable,
) -> Result<SwarmSpawnResult, &'static str> {
    println!();
    println!("  ╔══════════════════════════════════════════════════╗");
    println!("  ║     SwarmScope -> Oxide OS Agent Loader          ║");
    println!("  ╠══════════════════════════════════════════════════╣");
    if let Some(ref pid) = manifest.project_id {
        println!("  ║  Project:  {:<37} ║", pid);
    }
    if let Some(ref gid) = manifest.graph_id {
        println!("  ║  Graph:    {:<37} ║", gid);
    }
    println!("  ║  Agents:   {:<37} ║", manifest.agent_count);
    println!("  ║  Channels: {:<37} ║", manifest.channels.len());
    println!("  ╚══════════════════════════════════════════════════╝");
    println!();

    let broker_endpoint: String = manifest
        .broker_endpoint
        .clone()
        .unwrap_or_else(|| String::from(broker::DEFAULT_BROKER_ENDPOINT));
    println!("[swarmscope] broker endpoint: {}", broker_endpoint);

    // 1. Create all IPC channels
    println!("[swarmscope] Creating {} IPC channels...", manifest.channels.len());
    for ch_name in &manifest.channels {
        channel::create(ch_name.clone());
    }

    // 2. Find the supervisor agent
    let supervisor_idx = manifest
        .agents
        .iter()
        .position(|a| a.role == AgentRole::Supervisor)
        .ok_or("manifest has no supervisor agent")?;

    // 3. Create capabilities and spawn the supervisor first
    let sup_agent = &manifest.agents[supervisor_idx];
    let sup_task_id = spawn_manifest_agent(sup_agent, None, mapper)?;

    let mut agent_ids = Vec::new();
    agent_ids.push((sup_agent.name.clone(), sup_task_id));

    // 4. Spawn all other agents as children of the supervisor
    let mut cap_count = 0;
    for (idx, agent_def) in manifest.agents.iter().enumerate() {
        if idx == supervisor_idx {
            continue;
        }

        match spawn_manifest_agent(agent_def, Some(sup_task_id), mapper) {
            Ok(agent_id) => {
                agent_ids.push((agent_def.name.clone(), agent_id));

                // Create capabilities for this agent
                let mut table = CAP_TABLE.lock();
                for cap_def in &agent_def.capabilities {
                    let resource = to_resource_ref(&cap_def.resource);
                    let perms = to_permission_bits(&cap_def.permissions);
                    table.create_root(agent_id, resource, perms);
                    cap_count += 1;
                }
            }
            Err(e) => {
                println!(
                    "[swarmscope] WARNING: Failed to spawn '{}': {}",
                    agent_def.name, e
                );
            }
        }
    }

    // 4b. Auto-grant broker network cap to each individual/group_rep agent
    //     and persist endpoint + cap on the registry entry.
    for (agent_name, agent_id) in &agent_ids {
        let agent_def = match manifest.agents.iter().find(|a| a.name == *agent_name) {
            Some(d) => d,
            None => continue,
        };
        if matches!(agent_def.role, AgentRole::Individual | AgentRole::GroupRepresentative) {
            let cap = auto_grant_broker_cap(*agent_id, &broker_endpoint);
            let mut reg = agent::registry::REGISTRY.lock();
            if let Some(a) = reg.get_mut(*agent_id) {
                a.broker_cap = cap;
                a.broker_endpoint = broker_endpoint.clone();
            }
        }
    }

    // 4c. Setup per-agent pub/sub channels (agent.<name>.out) and grant caps.
    let publish_caps = setup_agent_channels(manifest, &agent_ids);

    // 4d. Store publish caps on the registry entries so the agent entry can use them.
    for (agent_name, cap_id) in &publish_caps {
        if let Some((_, agent_id)) = agent_ids.iter().find(|(n, _)| n == agent_name) {
            let mut reg = agent::registry::REGISTRY.lock();
            if let Some(a) = reg.get_mut(*agent_id) {
                a.publish_cap = Some(*cap_id);
            }
        }
    }

    // 5. Print the supervision tree
    println!();
    println!("[swarmscope] Agent supervision tree:");
    agent::registry::REGISTRY.lock().print_tree();

    // 6. Print summary
    println!();
    println!(
        "[swarmscope] Swarm loaded: {} agents, {} channels, {} capabilities",
        agent_ids.len(),
        manifest.channels.len(),
        cap_count
    );

    // Print SwarmScope metadata for simulation agents
    let sim_agents: Vec<&ManifestAgent> = manifest
        .agents
        .iter()
        .filter(|a| a.metadata.is_some())
        .collect();
    if !sim_agents.is_empty() {
        println!();
        println!("[swarmscope] Simulation agents ({}):", sim_agents.len());
        for a in &sim_agents {
            if let Some(ref meta) = a.metadata {
                let entity_type = meta
                    .source_entity_type
                    .as_deref()
                    .unwrap_or("unknown");
                let profession = meta.profession.as_deref().unwrap_or("-");
                println!(
                    "  {} [{}] {} age:{} prof:{}",
                    a.name,
                    entity_type,
                    a.role_str(),
                    meta.age.unwrap_or(0),
                    profession
                );
            }
        }
    }

    Ok(SwarmSpawnResult {
        supervisor_id: sup_task_id,
        agent_ids,
        channel_count: manifest.channels.len(),
        cap_count,
    })
}

/// Spawn a single agent from a manifest definition.
fn spawn_manifest_agent(
    agent_def: &ManifestAgent,
    parent_id: Option<AgentId>,
    mapper: &mut OffsetPageTable,
) -> Result<AgentId, &'static str> {
    let config = AgentConfig {
        name: agent_def.name.clone(),
        system_prompt: Some(agent_def.system_prompt.clone()),
        model: to_model_binding(&agent_def.model),
        tools: agent_def.tools.clone(),
        capabilities: Vec::new(), // Caps created separately via CAP_TABLE
        restart_policy: to_restart_policy(agent_def.restart_policy),
        resource_limits: ResourceLimits {
            max_memory_bytes: agent_def.resource_limits.max_memory_bytes,
            max_children: agent_def.resource_limits.max_children,
        },
    };

    let entry = entry_for_role(agent_def.role);
    agent::lifecycle::spawn(parent_id, config, entry, mapper)
}

// Helper for ManifestAgent display
impl ManifestAgent {
    pub fn role_str(&self) -> &'static str {
        match self.role {
            AgentRole::Supervisor => "supervisor",
            AgentRole::Individual => "individual",
            AgentRole::GroupRepresentative => "group-rep",
            AgentRole::Aggregator => "aggregator",
            AgentRole::Reporter => "reporter",
        }
    }
}
