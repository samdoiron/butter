use std::future::Future;
use std::thread;
use smol::{Executor, Task};
use std::panic::catch_unwind;
use once_cell::sync::Lazy;
use futures_lite::future;

pub fn spawn<T: Send + 'static>(future: impl Future<Output = T> + Send + 'static) -> Task<T> {
  static GLOBAL: Lazy<Executor> = Lazy::new(|| {
    for n in 1..4 {
        thread::Builder::new()
            .name(format!("butter-{}", n))
            .spawn(|| {
                loop {
                    let _ = catch_unwind(|| {
                        async_io::block_on(GLOBAL.run(future::pending::<()>()))
                    });
                }
            })
            .expect("cannot spawn executor thread");
    }

    Executor::new()
  });

  GLOBAL.spawn(future)
}
// 33.69s