//! Diamond DAG with the root triggered every 500ms.

use core::time::Duration;
use taktora_executor::{Executor, ItemFlow, item, item_with_triggers};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut exec = Executor::builder().worker_threads(4).build()?;

    let mut g = exec.add_graph();
    let root = g.vertex(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(500));
            Ok(())
        },
        |_| {
            println!("root");
            Ok(ItemFlow::Continue)
        },
    ));
    let left = g.vertex(item(|_| {
        println!("  left");
        Ok(ItemFlow::Continue)
    }));
    let right = g.vertex(item(|_| {
        println!("  right");
        Ok(ItemFlow::Continue)
    }));
    let merge = g.vertex(item(|_| {
        println!("merge");
        Ok(ItemFlow::Continue)
    }));
    g.edge(root, left);
    g.edge(root, right);
    g.edge(left, merge);
    g.edge(right, merge);
    g.root(root);
    g.build()?;

    exec.run()?;
    Ok(())
}
