use super::*;
use std::thread;
use wirebolt_core::ProxyPolicy;

/// Each test owns its cache, so resets in one test cannot race lookups
/// in another the way a shared static would.
fn engine_for(cache: &HttpEngineCache, proxy: &ResolvedProxy) -> Arc<HttpEngine> {
    cache
        .engine(proxy.mode(), || {
            HttpEngine::with_proxy(
                &HttpEngineConfig::default(),
                proxy,
                &wirebolt_core::NoSecrets,
            )
        })
        .expect("HTTP engine")
}

fn manual_proxy_mode() -> ProxyMode {
    let route = wirebolt_core::ProxyRoute::new(
        wirebolt_core::ProxyDestination::All,
        wirebolt_core::ProxyEndpoint::new("http://proxy.internal:8080").expect("proxy endpoint"),
    );
    ProxyMode::Manual(wirebolt_core::ManualProxy::new(vec![route]).expect("manual proxy"))
}

#[test]
fn shared_direct_engine_reuses_the_same_client_pool() {
    let cache = HttpEngineCache::default();
    let proxy = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);

    let first = engine_for(&cache, &proxy);
    let second = engine_for(&cache, &proxy);

    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn shared_manual_engine_is_keyed_by_mode_and_reset_on_demand() {
    let cache = HttpEngineCache::default();
    let manual = manual_proxy_mode();
    let from_workspace = ProxyPolicy::with_workspace(manual.clone()).resolve(None);
    let from_request = ProxyPolicy::default().resolve(Some(&manual));

    let first = engine_for(&cache, &from_workspace);
    let second = engine_for(&cache, &from_request);
    assert!(Arc::ptr_eq(&first, &second));

    cache.reset();
    let replacement = engine_for(&cache, &from_workspace);
    assert!(!Arc::ptr_eq(&first, &replacement));
}

#[test]
fn reset_retires_every_slot_at_once() {
    let cache = HttpEngineCache::default();
    let direct = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);
    let system = ProxyPolicy::with_workspace(ProxyMode::System).resolve(None);
    let manual = ProxyPolicy::with_workspace(manual_proxy_mode()).resolve(None);
    let before = [
        engine_for(&cache, &direct),
        engine_for(&cache, &system),
        engine_for(&cache, &manual),
    ];

    cache.reset();

    let after = [
        engine_for(&cache, &direct),
        engine_for(&cache, &system),
        engine_for(&cache, &manual),
    ];
    for (old, new) in before.iter().zip(&after) {
        assert!(!Arc::ptr_eq(old, new));
    }
    for (old, new) in after.iter().zip([
        engine_for(&cache, &direct),
        engine_for(&cache, &system),
        engine_for(&cache, &manual),
    ]) {
        assert!(Arc::ptr_eq(old, &new));
    }
}

#[test]
fn an_engine_built_while_a_reset_ran_is_used_once_but_never_pooled() {
    let cache = HttpEngineCache::default();
    let proxy = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);

    let raced = cache
        .engine(proxy.mode(), || {
            // Simulates a network change landing while the client is
            // still being built with the pre-change proxy settings.
            cache.reset();
            HttpEngine::with_proxy(
                &HttpEngineConfig::default(),
                &proxy,
                &wirebolt_core::NoSecrets,
            )
        })
        .expect("engine for this run");

    let pooled = engine_for(&cache, &proxy);
    assert!(!Arc::ptr_eq(&raced, &pooled));
    assert!(Arc::ptr_eq(&pooled, &engine_for(&cache, &proxy)));
}

#[test]
fn concurrent_lookups_and_resets_never_hand_out_a_retired_engine() {
    let cache = Arc::new(HttpEngineCache::default());
    let proxy = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let cache = Arc::clone(&cache);
            let proxy = proxy.clone();
            thread::spawn(move || {
                for _ in 0..50 {
                    drop(engine_for(&cache, &proxy));
                }
            })
        })
        .collect();
    for _ in 0..20 {
        cache.reset();
        thread::sleep(Duration::from_millis(1));
    }
    for worker in workers {
        worker.join().expect("worker thread");
    }

    let retired = engine_for(&cache, &proxy);
    cache.reset();
    let fresh = engine_for(&cache, &proxy);
    assert!(!Arc::ptr_eq(&retired, &fresh));
    assert!(Arc::ptr_eq(&fresh, &engine_for(&cache, &proxy)));
}

#[test]
fn zero_timeouts_disable_the_deadline() {
    assert_eq!(timeout_from_millis(0), None);
    assert_eq!(timeout_from_millis(250), Some(Duration::from_millis(250)));
}
