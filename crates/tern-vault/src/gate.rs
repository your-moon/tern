//! One scrypt run at a time. Each run needs a 256 MiB scratch buffer at the vault's work
//! factor, so two at once (a save racing a sync merge, say) would double the peak memory.

use std::sync::Mutex;

static SCRYPT: Mutex<()> = Mutex::new(());

/// Runs `work` (an age scrypt encrypt or decrypt) once no other is running in this process.
/// Never call it from inside `work`.
pub(crate) fn one_at_a_time<T>(work: impl FnOnce() -> T) -> T {
    // The guard holds no data, so a panic in another run leaves nothing to distrust.
    let _turn = SCRYPT.lock().unwrap_or_else(|e| e.into_inner());
    work()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::disallowed_methods)]
mod tests {
    use super::*;
    use crate::{SecretString, Vault};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn two_threads_never_run_scrypt_at_once() {
        let running = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    for _ in 0..3 {
                        one_at_a_time(|| {
                            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                            peak.fetch_max(now, Ordering::SeqCst);
                            std::thread::sleep(Duration::from_millis(5));
                            running.fetch_sub(1, Ordering::SeqCst);
                        });
                    }
                });
            }
        });
        assert_eq!(peak.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_panicking_run_does_not_wedge_the_gate() {
        let crashed = std::thread::spawn(|| one_at_a_time(|| panic!("boom"))).join();
        assert!(crashed.is_err());
        assert_eq!(one_at_a_time(|| 7), 7);
    }

    #[test]
    fn seal_and_open_still_round_trip_through_the_gate() {
        let v = Vault::new(SecretString::from("pw".to_owned())).with_work_factor(4);
        let sealed = v.seal(b"data").unwrap();
        assert_eq!(&**v.open(&sealed).unwrap(), b"data");
    }
}
