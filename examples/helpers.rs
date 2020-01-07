#[macro_export]
macro_rules! wait_or_panic {
    // Execute a block of code in a loop with 1 second waits up to 30 seconds total run time
    // Use: wait_or_panic!({if 5 > 2 { break }})
    ($code:block) => {

        {
            use std::time::{Instant, Duration};

            let start = Instant::now();
            loop {

                $code

                if Instant::now() - start > Duration::from_secs(30) {
                    panic!("Timeout waiting for condition");
                } else {
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }

    }
}
