//! Two items connected by a `Channel<u64>`. The first publishes a counter
//! every 200ms; the second consumes it.

use core::time::Duration;
use iceoryx2::prelude::*;
use taktora_executor::{Executor, ItemFlow, item_with_triggers};

#[derive(Debug, Default, Clone, Copy, ZeroCopySend)]
#[repr(C)]
struct Count(u64);

/// Consumer item that owns the subscriber so `declare_triggers` and `execute`
/// both have access without any double-move issue.
struct Consumer {
    sub: taktora_executor::Subscriber<Count>,
}

impl taktora_executor::ExecutableItem for Consumer {
    fn declare_triggers(
        &mut self,
        d: &mut taktora_executor::TriggerDeclarer<'_>,
    ) -> Result<(), taktora_executor::ExecutorError> {
        d.subscriber(&self.sub);
        Ok(())
    }

    fn execute(
        &mut self,
        _ctx: &mut taktora_executor::Context<'_>,
    ) -> taktora_executor::ExecuteResult {
        while let Some(s) = self
            .sub
            .take()
            .map_err(|e| -> taktora_executor::ItemError { Box::new(e) })?
        {
            println!("got {}", s.payload().0);
        }
        Ok(ItemFlow::Continue)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut exec = Executor::builder().worker_threads(2).build()?;
    let ch = exec.channel::<Count>("taktora.examples.pipeline")?;
    let publisher = ch.publisher()?;
    let subscriber = ch.subscriber()?;

    // Producer item.
    let mut n = 0_u64;
    exec.add(item_with_triggers(
        |d| {
            d.interval(Duration::from_millis(200));
            Ok(())
        },
        move |_| {
            let _ = publisher
                .send_copy(Count(n))
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;
            n += 1;
            Ok(ItemFlow::Continue)
        },
    ))?;

    // Consumer item reads every available message.
    exec.add(Consumer { sub: subscriber })?;

    exec.run()?;
    Ok(())
}
