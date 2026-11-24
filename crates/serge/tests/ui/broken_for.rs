use serge::prelude::*;

fn main() {
    let _ = ui! { Card { for in 0..3 { key: 1, Text { "a" } } } };
}
