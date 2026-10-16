// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Где окно Cutless держит свой сервер, если разрешило агентам править
//! открытый проект. Читаем `settings.json` своими полями, без крейта окна.

use std::net::SocketAddr;
use std::path::Path;

use serde::Deserialize;

/// Имя файла настроек в папке конфигурации.
const FILE: &str = "settings.json";

/// Только нужные поля `settings.json`: сервер и согласие на агентов.
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
    server: Server,
}

/// Настройки встроенного сервера окна.
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Server {
    enabled: bool,
    listen: String,
    token: String,
    agents: bool,
}

/// Адрес и токен окна, если оно включено и разрешило агентам правки.
pub fn window_endpoint(config_dir: &Path) -> Option<(SocketAddr, String)> {
    let bytes = std::fs::read(config_dir.join(FILE)).ok()?;
    let Settings { server } = serde_json::from_slice(&bytes).ok()?;
    if !server.enabled || !server.agents {
        return None;
    }
    let address: SocketAddr = server.listen.parse().ok()?;
    Some((address, server.token))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Временная папка с `settings.json` данного содержимого.
    fn with_settings(json: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(FILE), json).expect("write");
        dir
    }

    #[test]
    fn no_file_means_no_window() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(window_endpoint(dir.path()), None);
    }

    #[test]
    fn a_disabled_server_means_no_window() {
        let dir = with_settings(r#"{ "server": { "enabled": false, "agents": true } }"#);
        assert_eq!(window_endpoint(dir.path()), None);
    }

    #[test]
    fn agents_turned_off_means_no_window() {
        let dir = with_settings(
            r#"{ "server": { "enabled": true, "listen": "127.0.0.1:7420", "token": "t", "agents": false } }"#,
        );
        assert_eq!(window_endpoint(dir.path()), None);
    }

    #[test]
    fn enabled_with_agents_gives_the_address_and_token() {
        let dir = with_settings(
            r#"{ "server": { "enabled": true, "listen": "127.0.0.1:7420", "token": "secret", "agents": true } }"#,
        );
        let (address, token) = window_endpoint(dir.path()).expect("endpoint");
        assert_eq!(address, "127.0.0.1:7420".parse().expect("addr"));
        assert_eq!(token, "secret");
    }

    #[test]
    fn a_broken_address_is_no_window() {
        let dir = with_settings(
            r#"{ "server": { "enabled": true, "listen": "not an address", "token": "t", "agents": true } }"#,
        );
        assert_eq!(window_endpoint(dir.path()), None);
    }

    #[test]
    fn an_older_file_without_agents_means_no_window() {
        let dir = with_settings(
            r#"{ "server": { "enabled": true, "listen": "127.0.0.1:7420", "token": "t" } }"#,
        );
        assert_eq!(window_endpoint(dir.path()), None);
    }
}
