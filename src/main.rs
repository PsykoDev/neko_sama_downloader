#![feature(fs_try_exists)]

use std::{error::Error, time::Duration};
use std::str::FromStr;

use clap::Parser;
use requestty::{OnEsc, prompt_one, Question};

use mod_file::{
    {search, search::ProcessingUrl},
    {utils_data, utils_data::time_to_human_time}, chrome_spawn::ChromeChild,
    cmd_line_parser,
    cmd_line_parser::Scan, process_part1, process_part1::{add_ublock, connect_to_chrome_driver},
    static_data,
    thread_pool,
    utils_check,
};

mod mod_file;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut new_args = cmd_line_parser::Args::parse();
    header!("{}", static_data::HEADER);

    if new_args.url_or_search_word.is_empty() {
        warn!("prefers use ./{} -h", utils_data::exe_name());
        if let Ok(reply) = utils_data::ask_keyword("Enter url to direct download or keyword to search: ")
        {
            new_args.url_or_search_word = Scan::from_str(reply.as_string().unwrap().trim())?;
        }
    }

    info!("{}", new_args);

    let thread = thread_pool::max_thread_check(&new_args)?;

    let processing_url = match new_args.url_or_search_word {
        Scan::Search(ref keyword) => {
            let find =
                search::search_over_json(&keyword, &new_args.language, &new_args.debug).await?;

            let mut ep = 0;
            let mut film = 0;

            let _: Vec<_> = find
                .iter()
                .map(|s| {
                    if s.ep.starts_with("Film") {
                        film += 1;
                    } else {
                        ep +=
                            s.ep.split_whitespace()
                                .nth(0)
                                .unwrap()
                                .parse::<i32>()
                                .unwrap_or(1);
                    };
                })
                .collect();
            header!(
                "Seasons found: {} Episode found: {} ({}~ Go Total) Films found {} ({}~ Go Total)",
                find.len(),
                ep,
                ep * 250 / 1024,
                film,
                film * 1300 / 1024
            );

            let multi_select = Question::multi_select("Season")
                .message("What seasons do you want?")
                .choices(
                    find.iter()
                        .map(|s| {
                            let tmp_genre = s.clone().genre;
                            format!(
                                "{} ({})\n[{}]",
                                s.name,
                                s.ep,
                                if tmp_genre.is_empty() {
                                    String::from("no tag found")
                                } else {
                                    tmp_genre
                                }
                            )
                        })
                        .collect::<Vec<String>>(),
                )
                .on_esc(OnEsc::Terminate)
                .page_size(20)
                .should_loop(false)
                .build();
            let answer = prompt_one(multi_select)?;

            let matching_processing_urls: Vec<_> = answer
                .try_into_list_items()
                .unwrap()
                .iter()
                .filter_map(|number| find.get(number.index).cloned())
                .collect();

            matching_processing_urls
        }

        Scan::Download(ref url) => {
            vec![ProcessingUrl {
                name: "".to_string(),
                ep: "".to_string(),
                url: url.to_string(),
                genre: "".to_string(),
            }]
        }
    };

    let path = utils_check::confirm().await?;

    time_it!("Global time:", {
        if new_args.debug {
            debug!("spawn chrome process");
        }

        let mut child = ChromeChild::spawn(&path.chrome_path);
        if new_args.debug {
            debug!("wait 1sec chrome process spawn correctly");
        }
        tokio::time::sleep(Duration::from_secs(1)).await;

        for (index, x) in processing_url.iter().enumerate() {
            header!("Step {} / {}", index + 1, processing_url.len());
            info!("Process: {}", x.url);
            let driver = connect_to_chrome_driver(&new_args, add_ublock(&new_args, &path)?, &x.url).await?;
            process_part1::start(&x.url, &path, thread, &new_args, driver).await?;
        }

        child.chrome.kill()?;
    });

    Ok(())
}
