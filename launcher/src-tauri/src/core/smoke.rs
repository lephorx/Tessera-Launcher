//! Real end-to-end launches, run by hand (each opens a game window for a moment):
//! `cargo test --release smoke -- --ignored --nocapture --test-threads 1`

#[cfg(test)]
mod tests {
    use crate::core::instances::{self, Loader};
    use crate::core::launch::{prepare, Player};
    use crate::core::paths::Paths;
    use crate::core::{http_client, loaders};
    use tokio::io::{AsyncBufReadExt, BufReader};

    async fn launch(loader: Loader, game: &str) {
        let root = std::env::var("TESSERA_SMOKE_DIR").map(Into::into).unwrap_or_else(|_| std::env::temp_dir().join("tessera-smoke"));
        let paths = Paths::new(root);
        let client = http_client();
        let version = match loader {
            Loader::Vanilla => None,
            _ => {
                let list = loaders::versions(&client, loader, game).await.unwrap();
                Some(list.iter().find(|l| l.stable).or(list.first()).expect("loader versions").version.clone())
            }
        };
        println!("== {loader:?} {game} {version:?}");
        let inst = instances::create(&paths, &format!("Smoke {loader:?}"), game, loader, version, None).unwrap();
        let plan = prepare(&client, &paths, &inst, &Player::offline("TesseraTest"), 3072, None, &|stage, d, t| {
            if t == 0 || d == t {
                println!("{stage} {d}/{t}");
            }
        })
        .await
        .unwrap();

        if std::env::var("TESSERA_SMOKE_ARGS").is_ok() {
            std::fs::write("/tmp/tessera-smoke-cmd.txt", format!("{}\n{}", plan.java.display(), plan.args.join("\n"))).unwrap();
        }
        let mut child = tokio::process::Command::new(&plan.java)
            .args(&plan.args)
            .current_dir(&plan.game_dir)
            .env_remove("DYLD_FALLBACK_LIBRARY_PATH")
            .env_remove("DYLD_LIBRARY_PATH")
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let started = tokio::time::timeout(std::time::Duration::from_secs(300), async {
            while let Ok(Some(line)) = lines.next_line().await {
                println!("| {line}");
                if line.contains("Backend library: LWJGL") || line.contains("Sound engine started") {
                    return true;
                }
            }
            false
        })
        .await
        .unwrap_or(false);
        child.kill().await.ok();
        instances::delete(&paths, &inst.id).unwrap();
        assert!(started, "the game did not start");
    }

    /// Any version: TESSERA_SMOKE_GAME=1.20.1
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn smoke_vanilla() {
        launch(Loader::Vanilla, &std::env::var("TESSERA_SMOKE_GAME").unwrap_or_else(|_| "1.21.1".into())).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn smoke_fabric() {
        launch(Loader::Fabric, "1.21.1").await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn smoke_quilt() {
        launch(Loader::Quilt, "1.21.1").await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn smoke_neoforge() {
        launch(Loader::NeoForge, "1.21.1").await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn smoke_forge() {
        launch(Loader::Forge, "1.20.1").await;
    }
}
