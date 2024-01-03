#![feature(pattern)]
use crate::search::ProcessingUrl;
use clap::Parser;
use std::io::{stdin, stdout, Write};
use std::{error::Error, fs, process::exit, time::Instant};

mod cmd_line_parser;
mod html_parser;
mod log_color;
mod process_part1;
mod search;
mod static_data;
mod thread_pool;
mod utils_check;
mod utils_data;
mod vlc_playlist_builder;
mod web;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let new_args = cmd_line_parser::Args::parse();
    info!("Debug print is {}", new_args.debug);
    let path = utils_check::check()?;

    let mut chrome_check = false;
    let mut ffmpeg_check = false;
    let mut ublock_check = false;

    let mut thread = new_args.thread as usize;
    let max_thread = std::thread::available_parallelism()?.get() * 4;
    if thread > max_thread {
        warn!("Max thread for your cpu is between 1 and {}", max_thread);
        thread = max_thread;
    }

    let mut processing_url = vec![];
    match new_args.scan.as_str() {
        "search" => {
            let find = search::search_over_json(
                &new_args.url_or_search_word,
                &new_args.language,
                &new_args.debug,
            )
            .await?;

            processing_url.extend(find.clone());

            let mut nb_episodes = 0;
            if find.len() <= 50 {
                for (id, processing_url) in find.iter().enumerate() {
                    dl_ready!(
                        "({}): {} ({}):",
                        id + 1,
                        processing_url.name,
                        processing_url.ep
                    );
                    println!("{}\n", processing_url.url);
                    nb_episodes += processing_url
                        .ep
                        .split_whitespace()
                        .nth(0)
                        .unwrap()
                        .parse::<i32>()
                        .unwrap_or(1);
                }
            } else {
                for x in find {
                    nb_episodes +=
                        x.ep.split_whitespace()
                            .nth(0)
                            .unwrap()
                            .parse::<i32>()
                            .unwrap_or(1);
                }
                warn!("more than 50 seasons found")
            }
            let proc_len = processing_url.len();
            let mut s = String::new();

            if new_args.url_or_search_word != " " {
                print!("Ready to download ({proc_len}) seasons? 'Y' to download all, 'n' to cancel, or choose a season [1-{proc_len}]: ");
            } else {
                print!("Ready to download NekoSama ({}) entirely ? ({proc_len}) seasons ? so {nb_episodes} Eps  [Y/n]: ",new_args.language);
            }
            let _ = stdout().flush();
            stdin()
                .read_line(&mut s)
                .expect("Did not enter a correct string");
            if let Some('\n') = s.chars().next_back() {
                s.pop();
            }
            if let Some('\r') = s.chars().next_back() {
                s.pop();
            }
            if let Ok(mut pick) = s.parse::<usize>() {
                if pick <= 0 {
                    pick = 1;
                }
                if pick >= proc_len {
                    pick = proc_len;
                }

                let url = processing_url[pick - 1].clone();
                processing_url.clear();
                processing_url.append(&mut vec![url]);
            }
            if s == "n" {
                exit(0);
            }
        }
        "download" => {
            let x = ProcessingUrl {
                name: "".to_string(),
                ep: "".to_string(),
                url: new_args.url_or_search_word,
            };
            processing_url.extend(vec![x]);
        }
        _ => {
            warn!(
                "\"{}\" doesn't exist use search or download only",
                new_args.scan
            );
            exit(0);
        }
    }

    fs::create_dir_all(&path.extract_path)?;

    for entry in fs::read_dir(&path.extract_path)? {
        if let Ok(x) = entry {
            if let Some(file_name) = x.file_name().to_str() {
                #[cfg(target_os = "windows")]
                if file_name.ends_with(".exe") {
                    if file_name.contains("chromedriver") {
                        chrome_check = true;
                    }
                    if file_name.contains("ffmpeg") {
                        ffmpeg_check = true;
                    }
                }

                #[cfg(target_family = "unix")]
                if file_name.ends_with("") {
                    if file_name.contains("chromedriver") {
                        chrome_check = true;
                    }
                    if file_name.contains("ffmpeg") {
                        ffmpeg_check = true;
                    }
                }

                if file_name.ends_with(".crx") {
                    if file_name.contains("uBlock-Origin") {
                        ublock_check = true;
                    }
                }
            }
        }
    }
    info!("chromedriver is present\t? {chrome_check}");
    info!("ffmpeg is present\t\t? {ffmpeg_check}");
    info!("uBlock Origin is present\t? {ublock_check}");
    if !ublock_check {
        utils_check::download(static_data::UBLOCK_PATH, &path.ublock_destination)
            .await
            .expect("Erreur lors du téléchargement de uBlock Origin.");
    }

    match ffmpeg_check && chrome_check && ublock_check {
        true => {
            let global_time = Instant::now();
            for x in processing_url {
                info!("Process: {}", x.url);
                process_part1::start(
                    &x.url,
                    &path.exe_path.parent().unwrap(),
                    &path.tmp_dl,
                    &path.chrome_path,
                    &path.u_block_path,
                    &path.ffmpeg_path,
                    thread,
                    &new_args.debug,
                )
                .await?;
            }
            info!(
                "Global time: {}",
                utils_data::time_to_human_time(global_time)
            );
        }
        false => {
            if !ffmpeg_check && chrome_check {
                error!(
                    "Please download then extract {} ffmpeg here:\n{}",
                    path.ffmpeg_path.display(),
                    static_data::FFMPEG_PATH
                );
            } else if !chrome_check && ffmpeg_check {
                error!(
                    "Please download chrome wed driver then extract {} in utils folder here:\n{}",
                    path.chrome_path.display(),
                    static_data::DRIVER_PATH
                );
            } else {
                error!(
                    "Please download chrome wed driver then extract {} in utils folder here:\n{}",
                    path.chrome_path.display(),
                    static_data::DRIVER_PATH
                );
                println!();
                error!(
                    "Please download then extract {} ffmpeg here:\n{}",
                    path.ffmpeg_path.display(),
                    static_data::FFMPEG_PATH
                );
            }
        }
    }

    let _ = utils_data::kill_process();
    Ok(())
}
