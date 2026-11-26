//! The library index, a small SQLite file in `~/.local/share/steam-fan/library.db`.
//!
//! The folders on the shelf stay the truth: you can still copy games in by hand.
//! The database only remembers what was found, so searching and starting Steam
//! never wait for the NAS, plus things folders can't hold (which program starts
//! a game, the history of what happened). Delete it and it is rebuilt.

use crate::shelf::{Game, GroupScan, Platform};
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;

pub struct Db {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub ts: String,
    pub game: String,
    pub msg: String,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        std::fs::create_dir_all(path.parent().unwrap())?;
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS groups (key TEXT PRIMARY KEY, scan TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS programs (
                 id TEXT PRIMARY KEY, program TEXT NOT NULL, platform TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS art (
                 id TEXT PRIMARY KEY, sgdb_id INTEGER, checked TEXT NOT NULL DEFAULT (datetime('now')));
             CREATE TABLE IF NOT EXISTS renames (old TEXT PRIMARY KEY, new TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS events (
                 ts TEXT NOT NULL DEFAULT (datetime('now','localtime')), game TEXT NOT NULL, msg TEXT NOT NULL);",
        )?;
        Ok(Db { conn })
    }

    pub fn open_default() -> Result<Db> {
        Db::open(&crate::config::data_dir().join("library.db"))
    }

    pub fn in_memory() -> Result<Db> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE groups (key TEXT PRIMARY KEY, scan TEXT NOT NULL);
             CREATE TABLE programs (id TEXT PRIMARY KEY, program TEXT NOT NULL, platform TEXT NOT NULL);
             CREATE TABLE art (id TEXT PRIMARY KEY, sgdb_id INTEGER, checked TEXT NOT NULL DEFAULT (datetime('now')));
             CREATE TABLE renames (old TEXT PRIMARY KEY, new TEXT NOT NULL);
             CREATE TABLE events (ts TEXT NOT NULL DEFAULT (datetime('now','localtime')), game TEXT NOT NULL, msg TEXT NOT NULL);",
        )?;
        Ok(Db { conn })
    }

    pub fn groups(&self) -> Result<HashMap<String, GroupScan>> {
        let mut st = self.conn.prepare("SELECT key, scan FROM groups")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = HashMap::new();
        for row in rows {
            let (k, s) = row?;
            if let Ok(g) = serde_json::from_str(&s) {
                out.insert(k, g);
            }
        }
        Ok(out)
    }

    pub fn save_groups(&mut self, groups: &HashMap<String, GroupScan>) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM groups", [])?;
        for (k, g) in groups {
            tx.execute("INSERT INTO groups (key, scan) VALUES (?1, ?2)", params![k, serde_json::to_string(g)?])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// All games from the last shelf read, without touching the shelf.
    pub fn games(&self) -> Result<Vec<Game>> {
        let progs = self.programs()?;
        let mut all: Vec<Game> = self.groups()?.into_values().flat_map(|g| g.games).collect();
        for g in &mut all {
            if let Some((p, plat)) = progs.get(&g.id) {
                g.program = p.clone();
                g.platform = *plat;
            }
        }
        all.sort_by(|a, b| (a.kind, a.group.to_lowercase(), a.name.to_lowercase()).cmp(&(b.kind, b.group.to_lowercase(), b.name.to_lowercase())));
        Ok(all)
    }

    pub fn programs(&self) -> Result<HashMap<String, (String, Platform)>> {
        let mut st = self.conn.prepare("SELECT id, program, platform FROM programs")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
        let mut out = HashMap::new();
        for row in rows {
            let (id, p, plat) = row?;
            let plat = serde_json::from_str(&format!("\"{plat}\"")).unwrap_or(Platform::Windows);
            out.insert(id, (p, plat));
        }
        Ok(out)
    }

    /// Remember which program starts a game. An empty program means "looked, found nothing".
    pub fn set_program(&self, id: &str, program: &str, platform: Platform) -> Result<()> {
        let plat = serde_json::to_string(&platform)?.trim_matches('"').to_string();
        self.conn.execute(
            "INSERT INTO programs (id, program, platform) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET program = excluded.program, platform = excluded.platform",
            params![id, program, plat],
        )?;
        Ok(())
    }

    pub fn program(&self, id: &str) -> Result<Option<(String, Platform)>> {
        Ok(self.programs()?.remove(id))
    }

    pub fn forget_programs(&self) -> Result<()> {
        self.conn.execute("DELETE FROM programs", [])?;
        Ok(())
    }

    /// SteamGridDB id found for a game, and whether it was looked up at all (None: never).
    pub fn art_lookup(&self, id: &str) -> Option<Option<u64>> {
        self.conn
            .query_row("SELECT sgdb_id FROM art WHERE id = ?1", params![id], |r| r.get::<_, Option<i64>>(0))
            .optional()
            .ok()
            .flatten()
            .map(|v| v.map(|x| x as u64))
    }

    pub fn set_art_lookup(&self, id: &str, sgdb: Option<u64>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO art (id, sgdb_id) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET sgdb_id = excluded.sgdb_id, checked = datetime('now')",
            params![id, sgdb.map(|v| v as i64)],
        )?;
        Ok(())
    }

    /// A game got a new id (moved or renamed): carry its program, artwork lookup and
    /// history over, and remember the change so Steam's entry can follow it.
    pub fn rename_game(&mut self, old: &str, new: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM programs WHERE id = ?2", params![old, new])?;
        tx.execute("UPDATE programs SET id = ?2 WHERE id = ?1", params![old, new])?;
        tx.execute("DELETE FROM art WHERE id = ?2", params![old, new])?;
        tx.execute("UPDATE art SET id = ?2 WHERE id = ?1", params![old, new])?;
        tx.execute("UPDATE events SET game = ?2 WHERE game = ?1", params![old, new])?;
        // a game renamed twice before Steam saw the first rename
        tx.execute("UPDATE renames SET new = ?2 WHERE new = ?1", params![old, new])?;
        tx.execute("INSERT OR REPLACE INTO renames (old, new) VALUES (?1, ?2)", params![old, new])?;
        tx.commit()?;
        Ok(())
    }

    /// Renames Steam hasn't been told about yet (old id -> new id).
    pub fn renames(&self) -> Result<HashMap<String, String>> {
        let mut st = self.conn.prepare("SELECT old, new FROM renames")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn clear_renames(&self) -> Result<()> {
        self.conn.execute("DELETE FROM renames", [])?;
        Ok(())
    }

    pub fn log(&self, game: &str, msg: &str) {
        let _ = self.conn.execute("INSERT INTO events (game, msg) VALUES (?1, ?2)", params![game, msg]);
    }

    pub fn events(&self, game: Option<&str>, limit: usize) -> Result<Vec<Event>> {
        let sql = "SELECT ts, game, msg FROM events WHERE (?1 IS NULL OR game = ?1) ORDER BY rowid DESC LIMIT ?2";
        let mut st = self.conn.prepare(sql)?;
        let rows = st.query_map(params![game, limit as i64], |r| Ok(Event { ts: r.get(0)?, game: r.get(1)?, msg: r.get(2)? }))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn last_event(&self, game: &str) -> Option<Event> {
        self.conn
            .query_row(
                "SELECT ts, game, msg FROM events WHERE game = ?1 ORDER BY rowid DESC LIMIT 1",
                params![game],
                |r| Ok(Event { ts: r.get(0)?, game: r.get(1)?, msg: r.get(2)? }),
            )
            .optional()
            .ok()
            .flatten()
    }
}
