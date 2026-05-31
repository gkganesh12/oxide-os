//! SwarmScope manifest data structures and builder.
//!
//! Mirrors the swarmscope_bridge_schema.json format using Rust types.
//! In production, a userspace service or host tool parses the JSON manifest
//! and calls the builder API. For demos, manifests are constructed inline.

use alloc::string::String;
use alloc::vec::Vec;

/// Permission flags matching the JSON schema permission strings.
/// Maps to kernel PermissionBits during loading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Read,
    Write,
    Execute,
    Delegate,
    Spawn,
    Kill,
    Subscribe,
    Publish,
    Connect,
}

/// Resource reference in the manifest, maps to kernel ResourceRef.
#[derive(Debug, Clone)]
pub enum ManifestResource {
    Channel { name: String },
    Network { host: String, port: u16 },
    Storage { path: String },
    Tool { name: String },
    Model { model_id: String },
    Agent { agent_id: u64 },
    AgentSpawn,
}

/// A single capability entry for an agent.
#[derive(Debug, Clone)]
pub struct ManifestCapability {
    pub resource: ManifestResource,
    pub permissions: Vec<Permission>,
}

/// Model binding configuration.
#[derive(Debug, Clone)]
pub enum ManifestModel {
    Local { model_id: String },
    Remote { endpoint: String },
    Auto { preference: Vec<String> },
}

/// Restart policy matching kernel RestartPolicy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestRestartPolicy {
    RestartOne,
    RestartAll,
    Escalate,
    Permanent,
}

/// Agent role determines default behavior and capability template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRole {
    Supervisor,
    Individual,
    GroupRepresentative,
    Aggregator,
    Reporter,
}

/// Resource limits for an agent.
#[derive(Debug, Clone)]
pub struct ManifestResourceLimits {
    pub max_memory_bytes: u64,
    pub max_children: usize,
}

impl Default for ManifestResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: 16 * 1024 * 1024,
            max_children: 0,
        }
    }
}

/// SwarmScope metadata preserved from the original profile.
#[derive(Debug, Clone)]
pub struct SwarmScopeMetadata {
    pub source_entity_uuid: Option<String>,
    pub source_entity_type: Option<String>,
    pub user_id: u64,
    pub age: Option<u32>,
    pub gender: Option<String>,
    pub mbti: Option<String>,
    pub country: Option<String>,
    pub profession: Option<String>,
    pub interested_topics: Vec<String>,
}

/// A single agent definition in the manifest.
#[derive(Debug, Clone)]
pub struct ManifestAgent {
    pub name: String,
    pub system_prompt: String,
    pub bio: String,
    pub role: AgentRole,
    pub model: ManifestModel,
    pub tools: Vec<String>,
    pub capabilities: Vec<ManifestCapability>,
    pub restart_policy: ManifestRestartPolicy,
    pub resource_limits: ManifestResourceLimits,
    pub metadata: Option<SwarmScopeMetadata>,
}

/// Supervision tree configuration.
#[derive(Debug, Clone)]
pub struct ManifestSupervision {
    pub root_agent: String,
    pub default_policy: ManifestRestartPolicy,
    pub max_restarts: u32,
}

/// Complete swarm manifest — the bridge between SwarmScope and Oxide OS.
#[derive(Debug, Clone)]
pub struct SwarmManifest {
    pub agents: Vec<ManifestAgent>,
    pub channels: Vec<String>,
    pub supervision: ManifestSupervision,
    pub project_id: Option<String>,
    pub graph_id: Option<String>,
    pub agent_count: usize,
    pub broker_endpoint: Option<String>,
}

/// Builder for constructing SwarmManifest programmatically.
pub struct ManifestBuilder {
    agents: Vec<ManifestAgent>,
    channels: Vec<String>,
    supervision: ManifestSupervision,
    project_id: Option<String>,
    graph_id: Option<String>,
    broker_endpoint: Option<String>,
}

impl ManifestBuilder {
    pub fn new() -> Self {
        Self {
            agents: Vec::new(),
            channels: Vec::new(),
            supervision: ManifestSupervision {
                root_agent: String::from("swarm-supervisor"),
                default_policy: ManifestRestartPolicy::RestartOne,
                max_restarts: 5,
            },
            project_id: None,
            graph_id: None,
            broker_endpoint: None,
        }
    }

    pub fn project_id(mut self, id: String) -> Self {
        self.project_id = Some(id);
        self
    }

    pub fn graph_id(mut self, id: String) -> Self {
        self.graph_id = Some(id);
        self
    }

    pub fn broker_endpoint(mut self, endpoint: String) -> Self {
        self.broker_endpoint = Some(endpoint);
        self
    }

    pub fn channel(mut self, name: String) -> Self {
        self.channels.push(name);
        self
    }

    pub fn channels(mut self, names: Vec<String>) -> Self {
        self.channels.extend(names);
        self
    }

    pub fn supervision(mut self, sup: ManifestSupervision) -> Self {
        self.supervision = sup;
        self
    }

    pub fn agent(mut self, agent: ManifestAgent) -> Self {
        self.agents.push(agent);
        self
    }

    /// Add a supervisor agent with standard capabilities.
    pub fn add_supervisor(mut self, max_children: usize) -> Self {
        let agent = ManifestAgent {
            name: String::from("swarm-supervisor"),
            system_prompt: String::from(
                "You are the swarm supervisor. Monitor all child agents, \
                 handle restarts on failure, and report aggregate status.",
            ),
            bio: String::from("Swarm supervisor"),
            role: AgentRole::Supervisor,
            model: ManifestModel::Auto {
                preference: alloc::vec![String::from("gpt-4")],
            },
            tools: Vec::new(),
            capabilities: alloc::vec![ManifestCapability {
                resource: ManifestResource::AgentSpawn,
                permissions: alloc::vec![Permission::Spawn, Permission::Kill, Permission::Delegate],
            }],
            restart_policy: ManifestRestartPolicy::Permanent,
            resource_limits: ManifestResourceLimits {
                max_memory_bytes: 32 * 1024 * 1024,
                max_children,
            },
            metadata: None,
        };
        self.agents.push(agent);
        self
    }

    /// Add an aggregator agent that subscribes to platform feeds.
    pub fn add_aggregator(mut self, platform_channels: &[&str]) -> Self {
        let mut caps = Vec::new();
        for ch in platform_channels {
            caps.push(ManifestCapability {
                resource: ManifestResource::Channel {
                    name: String::from(*ch),
                },
                permissions: alloc::vec![Permission::Subscribe],
            });
        }
        caps.push(ManifestCapability {
            resource: ManifestResource::Channel {
                name: String::from("agent-findings"),
            },
            permissions: alloc::vec![Permission::Subscribe],
        });
        caps.push(ManifestCapability {
            resource: ManifestResource::Channel {
                name: String::from("reports"),
            },
            permissions: alloc::vec![Permission::Publish],
        });
        caps.push(ManifestCapability {
            resource: ManifestResource::Storage {
                path: String::from("/simulation/aggregated"),
            },
            permissions: alloc::vec![Permission::Read, Permission::Write],
        });

        let agent = ManifestAgent {
            name: String::from("swarm-aggregator"),
            system_prompt: String::from(
                "Collect all agent posts and interactions. \
                 Produce periodic summaries and publish to the reports channel.",
            ),
            bio: String::from("Aggregates agent activity"),
            role: AgentRole::Aggregator,
            model: ManifestModel::Auto {
                preference: alloc::vec![String::from("gpt-4")],
            },
            tools: Vec::new(),
            capabilities: caps,
            restart_policy: ManifestRestartPolicy::RestartOne,
            resource_limits: ManifestResourceLimits {
                max_memory_bytes: 64 * 1024 * 1024,
                max_children: 0,
            },
            metadata: None,
        };
        self.agents.push(agent);
        self
    }

    /// Add a SwarmScope-generated agent (from a profile).
    pub fn add_swarmscope_agent(
        mut self,
        name: String,
        persona: String,
        bio: String,
        role: AgentRole,
        model_endpoint: String,
        platform_channels: &[&str],
        metadata: SwarmScopeMetadata,
    ) -> Self {
        let mut caps = Vec::new();

        // Platform channel access
        for ch in platform_channels {
            caps.push(ManifestCapability {
                resource: ManifestResource::Channel {
                    name: String::from(*ch),
                },
                permissions: alloc::vec![Permission::Publish, Permission::Subscribe],
            });
        }

        // Findings channel
        caps.push(ManifestCapability {
            resource: ManifestResource::Channel {
                name: String::from("agent-findings"),
            },
            permissions: alloc::vec![Permission::Publish],
        });

        // Network access to LLM
        let host = model_endpoint
            .split("//")
            .nth(1)
            .unwrap_or("api.openai.com")
            .split('/')
            .next()
            .unwrap_or("api.openai.com");
        caps.push(ManifestCapability {
            resource: ManifestResource::Network {
                host: String::from(host),
                port: 443,
            },
            permissions: alloc::vec![Permission::Connect],
        });

        // Group representatives get storage
        if role == AgentRole::GroupRepresentative {
            caps.push(ManifestCapability {
                resource: ManifestResource::Storage {
                    path: alloc::format!("/agents/{}/official", metadata.user_id),
                },
                permissions: alloc::vec![Permission::Read, Permission::Write],
            });
        }

        let mut tools = alloc::vec![
            String::from("social-post"),
            String::from("social-react"),
        ];
        if role == AgentRole::GroupRepresentative {
            tools.push(String::from("official-statement"));
        }

        let agent = ManifestAgent {
            name,
            system_prompt: persona,
            bio,
            role,
            model: ManifestModel::Remote {
                endpoint: model_endpoint,
            },
            tools,
            capabilities: caps,
            restart_policy: ManifestRestartPolicy::RestartOne,
            resource_limits: ManifestResourceLimits::default(),
            metadata: Some(metadata),
        };
        self.agents.push(agent);
        self
    }

    pub fn build(self) -> SwarmManifest {
        let count = self.agents.len();
        SwarmManifest {
            agents: self.agents,
            channels: self.channels,
            supervision: self.supervision,
            project_id: self.project_id,
            graph_id: self.graph_id,
            agent_count: count,
            broker_endpoint: self.broker_endpoint,
        }
    }
}
