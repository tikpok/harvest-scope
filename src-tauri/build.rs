use std::{fs, path::Path};

use sha2::{Digest, Sha384};

const IMMUTABLE_MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_initial.sql",
        "557B7AAC9EE1137FAF96715557899855F69754ED55819FCA88F39E60C786C831C096001E5157D7185FBD0EBA5D5F7234",
    ),
    (
        "0002_node_sort_order.sql",
        "31686A62FB0C0CA1E153F160F765FEAA811F0C5C0A9C17B21FC229ACD6DDDD73602EB6CBE91081C15E37AE2EE17A9D30",
    ),
    (
        "0003_diagnostics.sql",
        "06C8DFF46D5C865A9E43BC0155EBEE2A85004B705256FCD8DE7B7F30AD99F68DD0B4A0A8B39463E6FE33743E59320D30",
    ),
    (
        "0004_bridge_monitoring.sql",
        "7E7DD39EC214CB9B33516D9ADF9A4F841EEEBAA03C9DF7F07FCC07B3C3EA8B6B756BA70712BECAB4FEFE92DB51D3B1FA",
    ),
    (
        "0005_harvest_monitoring.sql",
        "1A483CD8C7528424534C24A4E94A8BB9EDE921A1EFDB0AA5F991AE731968C2F5EA2991EDB2FD81839D118581BFCAEE3A",
    ),
];

fn verify_immutable_migrations() {
    let migration_dir = Path::new("migrations");
    let mut actual = fs::read_dir(migration_dir)
        .expect("migrationsディレクトリを読み取れません")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|value| value.to_str()) == Some("sql"))
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
    actual.sort();
    let expected = IMMUTABLE_MIGRATIONS
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        actual, expected,
        "マイグレーション履歴が台帳と一致しません。適用済みSQLは削除・変更せず、追加時はbuild.rsの台帳も更新してください"
    );

    for (name, expected_checksum) in IMMUTABLE_MIGRATIONS {
        let path = migration_dir.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let contents = fs::read(&path)
            .unwrap_or_else(|error| panic!("{}を読み取れません: {error}", path.display()));
        let actual_checksum = format!("{:X}", Sha384::digest(contents));
        assert_eq!(
            actual_checksum, *expected_checksum,
            "{name}は適用後に変更されています。既存SQLを戻し、新しい変更は次番号のmigrationへ追加してください"
        );
    }
}

fn main() {
    verify_immutable_migrations();
    tauri_build::build()
}
