use serge::prelude::*;

#[component]
fn Badge(label: String) -> Element {
    ui! { Text { "{label}" } }
}

fn main() {
    let _ = ui! { Badge { label: "new", Text { "x" } } };
}
