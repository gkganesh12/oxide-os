use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;
use spin::Lazy;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
const STACK_SIZE: usize = 4096 * 5;

/// Double-fault stack — uses a static array with a wrapper to avoid `static mut`.
#[repr(C, align(4096))]
struct Stack([u8; STACK_SIZE]);
static DOUBLE_FAULT_STACK: Stack = Stack([0; STACK_SIZE]);

static TSS: Lazy<TaskStateSegment> = Lazy::new(|| {
    let mut tss = TaskStateSegment::new();
    tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
        let stack_start = VirtAddr::from_ptr(&DOUBLE_FAULT_STACK.0);
        stack_start + STACK_SIZE as u64
    };
    tss
});

static GDT: Lazy<(GlobalDescriptorTable, Selectors)> = Lazy::new(|| {
    let mut gdt = GlobalDescriptorTable::new();
    let code_selector = gdt.append(Descriptor::kernel_code_segment());   // Index 1 → 0x08
    let data_selector = gdt.append(Descriptor::kernel_data_segment());   // Index 2 → 0x10
    let _user_data_selector = gdt.append(Descriptor::user_data_segment()); // Index 3 → 0x1B (ring 3)
    let _user_code_selector = gdt.append(Descriptor::user_code_segment()); // Index 4 → 0x23 (ring 3)
    let tss_selector = gdt.append(Descriptor::tss_segment(&TSS));        // Index 5-6 (16-byte)
    (gdt, Selectors { code_selector, data_selector, tss_selector })
});

struct Selectors {
    code_selector: SegmentSelector,
    data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

pub fn init() {
    use x86_64::instructions::segmentation::{CS, DS, SS, Segment};
    use x86_64::instructions::tables::load_tss;

    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.code_selector);
        DS::set_reg(GDT.1.data_selector);
        SS::set_reg(SegmentSelector(0));
        load_tss(GDT.1.tss_selector);
    }
}
