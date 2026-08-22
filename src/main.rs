use reqwest::Client;
use reqwest_middleware::ClientBuilder;
use rodio::Decoder;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use std::process::exit;
use stream_download::http::HttpStream;
use stream_download::storage::adaptive::AdaptiveStorageProvider;
use stream_download::storage::temp::TempStorageProvider;
use stream_download::{Settings, StreamDownload};

#[derive(Serialize, Deserialize, Clone)]
struct UserConfig {
    volume: f32,
    prefetch: u64,
    software_ua_header: String,
    mp3_stream_url: String,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            volume: 0.85,
            prefetch: 0,
            software_ua_header:
                "Mozilla/5.0 (X11; Linux x86_64; rv:147.0) Gecko/20100101 Firefox/147.0".to_owned(),
            mp3_stream_url: "https://icast.connectmedia.hu/5201/live.mp3".to_owned(),
        }
    }
}

#[tokio::main]
async fn main() -> () {
    println!("Initializing stream listener..");
    let mut cfg: UserConfig =
        confy::load(std::env!("CARGO_PKG_NAME"), None).expect("Failed to load user configuration");

    let stream_handle = rodio::OutputStreamBuilder::open_default_stream()
        .expect("Failed to open default audio stream");
    let sink = rodio::Sink::connect_new(stream_handle.mixer());

    async fn create_audio_stream_from_web(
        cfg: &UserConfig,
    ) -> StreamDownload<AdaptiveStorageProvider<TempStorageProvider, TempStorageProvider>> {
        let reqwest_client = Client::builder()
            .user_agent(cfg.software_ua_header.as_str())
            .build()
            .expect("Failed to create reqwest client");

        let client = ClientBuilder::new(reqwest_client).build();

        let settings = Settings::default().prefetch_bytes(cfg.prefetch);
        let stream = HttpStream::new(
            client,
            cfg.mp3_stream_url
                .parse()
                .expect("Failed to parse mp3_stream_url from in-memory configuration"),
        )
        .await
        .expect("Failed to create HTTP stream");

        StreamDownload::from_stream(
            stream,
            AdaptiveStorageProvider::new(
                TempStorageProvider::new(),
                NonZeroUsize::new(512 * 1024)
                    .expect("Failed to create NonZeroUsize for adaptive storage provider"),
            ),
            settings,
        )
        .await
        .expect("Failed to initialize stream download")
    }

    let reader = create_audio_stream_from_web(&cfg).await;
    let source =
        Decoder::new_mp3(reader).expect("Failed to create MP3 decoder for the audio stream");

    sink.append(source);
    sink.set_volume(cfg.volume);

    println!("Started playback! Please use \"h\" for a list of commands.");
    let bad_command =
        || eprintln!("Unknown or invalid command. Please use \"h\" for a list of commands.");
    let mut rl = DefaultEditor::new().expect("Failed to initialize command line editor");

    let nice_exit = || -> ! {
        println!("Quitting, goodbye!");
        exit(0);
    };

    loop {
        let read_line = match rl.readline(">> ") {
            Ok(v) => v,
            Err(ReadlineError::Interrupted) => nice_exit(),
            Err(e) => panic!("Couldn't readline {}", e),
        };
        let mut parts = read_line.split_whitespace();
        let command = match parts.next() {
            Some(cmd) => cmd,
            None => {
                bad_command();
                continue;
            }
        };

        match command {
            "v" => {
                let volume = match parts.next() {
                    Some(vol) => vol,
                    None => {
                        println!("Volume is currently at: {:.2}%.", cfg.volume * 100.0);
                        continue;
                    }
                };

                let converted_float_volume = match volume.parse::<f32>() {
                    Ok(vol) => f32::max(0f32, vol / 100f32),
                    Err(_) => {
                        bad_command();
                        continue;
                    }
                };

                sink.set_volume(converted_float_volume);
                cfg.volume = converted_float_volume;
                println!("Volume set to: {:.2}%.", converted_float_volume * 100.0);
            }
            "cfg" => match confy::get_configuration_file_path(std::env!("CARGO_PKG_NAME"), None) {
                Ok(path) => println!("{}", path.display()),
                Err(_) => eprintln!("Failed to get config path"),
            },
            "p" => {
                sink.clear();
                println!("Paused playback.");
            }
            "s" => {
                if !sink.is_paused() {
                    sink.clear();
                }

                let reader = create_audio_stream_from_web(&cfg).await;
                let source = Decoder::new_mp3(reader)
                    .expect("Failed to create MP3 decoder for the audio stream");
                sink.append(source);

                sink.play();
                println!("Started playback.");
            }
            "h" => {
                println!(
                    r#"Commands:
    v <value>  - Set volume (e.g., 10.34 for 10.34%)
    p          - Pause playback
    s          - Start/resume playback
    q          - Quit
    w          - Write current settings to config
    cfg        - Show config path"#
                );
            }
            "q" => nice_exit(),
            "w" => match confy::store(std::env!("CARGO_PKG_NAME"), None, &cfg) {
                Ok(_) => println!("Successfully written config."),
                Err(_) => eprintln!("Failed to write config, settings aren't saved."),
            },
            _ => bad_command(),
        }
    }
}
