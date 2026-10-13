#[path = "support/control_flow.rs"]
mod control_flow;

use razor_slices::Slice;

fn main() {
    let groups = control_flow::sample_groups();
    let page = control_flow::grouped_tasks(&groups);
    let output = page.render_to_string().unwrap();
    assert_eq!(page.render_to_string().unwrap(), output);
    print!("{output}");
}
