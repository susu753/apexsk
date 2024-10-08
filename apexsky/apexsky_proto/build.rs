fn main() {
    tonic_build::configure()
        .build_client(true)
        .build_server(true)
        .out_dir("src/pb")
        .type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")
        .compile_protos(
            &[
                "proto/com/chettoy/apexsky/apexlegends/aimbot.proto",
                "proto/com/chettoy/apexsky/apexlegends/esp_data.proto",
                "proto/com/chettoy/apexsky/apexlegends/player.proto",
                "proto/com/chettoy/apexsky/apexlegends/spectator.proto",
                "proto/com/chettoy/apexsky/esp/esp_service.proto",
            ],
            &["proto"],
        )
        .unwrap();
}
