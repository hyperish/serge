use serge::prelude::*;

#[component]
fn Greeting(name: String, waved: Callback) -> Element {
    let _ = waved;
    ui! { Text { "Hello {name}" } }
}

fn main() {
    let _ = ui! { Greeting { } };
}
