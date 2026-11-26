//! Durable sub edits. Mutations run inside the config file transaction.
use crate::{config::Config, file_io};
use anyhow::Result;
use flock_net::{DirRules, MasterSettings, Role};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::{Path, PathBuf}};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    master: String,
    pub keys: BTreeMap<String, DirRules>,
}
fn path(config: &Path) -> PathBuf { config.with_file_name("key-rules.request") }
fn master(cfg: &Config) -> Option<&str> {
    (cfg.network.role == Role::Sub).then(|| cfg.network.peers.iter().find(|p| p.role == Role::Master).map(|p| p.id.as_str())).flatten()
}

pub fn queue(config: &Path, cfg: &Config) -> Result<()> {
    if let Some(master) = master(cfg) {
        let pending = Pending { master: master.into(), keys: cfg.network.all_rules() };
        file_io::atomic_write(&path(config), &serde_json::to_vec(&pending)?)?;
    }
    Ok(())
}

pub fn pending(config: &Path, cfg: &Config) -> Result<Option<Pending>> {
    let bytes = match std::fs::read(path(config)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored { Current(Pending), Legacy(BTreeMap<String, DirRules>) }
    let edit = match serde_json::from_slice(&bytes)? {
        Stored::Current(edit) => edit,
        Stored::Legacy(keys) => {
            let Some(master) = master(cfg) else { return Ok(None) };
            Pending { master: master.into(), keys }
        }
    };
    Ok((Some(edit.master.as_str()) == master(cfg)).then_some(edit))
}

/// Drops the outbox: unpair, role change, reset, or a master that cannot take edits.
pub fn clear(config: &Path) {
    match std::fs::remove_file(path(config)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("clear pending key rules: {e}"),
    }
}

/// Removes an outbox that no longer belongs to the config (not a sub any more, or another
/// master). Unreadable files go too.
pub fn drop_stale(config: &Path, cfg: &Config) {
    if path(config).exists() && !matches!(pending(config, cfg), Ok(Some(_))) {
        clear(config);
    }
}

/// The master's snapshot is the acknowledgement. Local pending rules win until it matches.
pub fn apply_master(config: &Path, cfg: &mut Config, settings: &MasterSettings) -> Result<()> {
    let edit = pending(config, cfg)?;
    settings.apply_to(&mut cfg.network);
    if let Some(edit) = edit {
        if settings.keys == edit.keys {
            std::fs::remove_file(path(config))?;
        } else {
            cfg.network.keys = edit.keys;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture { dir: PathBuf, config: PathBuf, cfg: Config }
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("flock-outbox-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::create_dir_all(&dir).unwrap();
            let mut cfg = Config::default();
            cfg.network.role = Role::Sub;
            cfg.network.peers.push(flock_net::PeerConfig { id: "master".into(), role: Role::Master, ..Default::default() });
            cfg.network.migrate_keys();
            Self { config: dir.join("config.toml"), dir, cfg }
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.dir); } }

    #[test]
    fn offline_request_survives_reads_restart_and_old_master_settings() {
        let mut f = Fixture::new();
        let old = MasterSettings::of(&f.cfg.network, "master");
        f.cfg.network.keys.get_mut("mac>windows").unwrap().hanja = true;
        queue(&f.config, &f.cfg).unwrap();
        let wanted = f.cfg.network.keys.clone();
        assert_eq!(pending(&f.config, &f.cfg).unwrap().unwrap().keys, wanted);
        assert_eq!(pending(&f.config, &f.cfg).unwrap().unwrap().keys, wanted);
        apply_master(&f.config, &mut f.cfg, &old).unwrap();
        assert_eq!(f.cfg.network.keys, wanted);
        assert!(path(&f.config).exists());
        let ack = MasterSettings::of(&f.cfg.network, "master");
        apply_master(&f.config, &mut f.cfg, &ack).unwrap();
        assert!(!path(&f.config).exists());
    }

    #[test]
    fn pending_requests_from_the_previous_format_remain_deliverable() {
        let mut f = Fixture::new();
        std::fs::write(path(&f.config), serde_json::to_vec(&f.cfg.network.keys).unwrap()).unwrap();
        assert_eq!(pending(&f.config, &f.cfg).unwrap().unwrap().keys, f.cfg.network.keys);
        assert!(path(&f.config).exists());
        let ack = MasterSettings::of(&f.cfg.network, "master");
        apply_master(&f.config, &mut f.cfg, &ack).unwrap();
        assert!(!path(&f.config).exists());
    }

    #[test]
    fn outbox_is_dropped_when_it_no_longer_fits_the_config() {
        let mut f = Fixture::new();
        queue(&f.config, &f.cfg).unwrap();
        drop_stale(&f.config, &f.cfg);
        assert!(path(&f.config).exists());
        f.cfg.network.peers.clear();
        drop_stale(&f.config, &f.cfg);
        assert!(!path(&f.config).exists());
        let mut g = Fixture::new();
        queue(&g.config, &g.cfg).unwrap();
        g.cfg.network.role = Role::Master;
        drop_stale(&g.config, &g.cfg);
        assert!(!path(&g.config).exists());
        let h = Fixture::new();
        queue(&h.config, &h.cfg).unwrap();
        clear(&h.config);
        clear(&h.config);
        assert!(!path(&h.config).exists());
    }

    #[test]
    fn old_echo_cannot_remove_a_newer_edit() {
        let mut f = Fixture::new();
        queue(&f.config, &f.cfg).unwrap();
        let old_echo = MasterSettings::of(&f.cfg.network, "master");
        let _read_by_sender = pending(&f.config, &f.cfg).unwrap();
        f.cfg.network.keys.get_mut("mac>windows").unwrap().hanja = true;
        queue(&f.config, &f.cfg).unwrap();
        apply_master(&f.config, &mut f.cfg, &old_echo).unwrap();
        assert!(path(&f.config).exists());
        assert!(f.cfg.network.keys["mac>windows"].hanja);
    }
}
