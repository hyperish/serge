use hyve::prelude::*;
use serge::prelude::*;

fn main() {
    let items = signal(vec![1, 2, 3]);
    let _ = ui! {
        Stack {
            for n in items() {
                Text { "{n}" }
            }
        }
    };
}
