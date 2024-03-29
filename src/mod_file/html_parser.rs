use std::{error::Error, fs::File, io, path::PathBuf};

use m3u8_rs::Playlist;
use reqwest::{Client, StatusCode};
use thirtyfour::{By, WebDriver, WebElement};

use crate::{debug, error, info, warn};
use crate::mod_file::{
    cmd_line_parser::Args, static_data::BASE_URL, utils_check::AllPath, utils_data, web,
};

pub async fn recursive_find_url(driver: &WebDriver, _url_test: &str, args: &Args, client: &Client, path: &AllPath) -> Result<(u16, u16), Box<dyn Error>> {
    let mut all_l = vec![];

    // direct url
    if _url_test.contains("/episode/") {
        driver.goto(_url_test).await?;
        all_l.push(_url_test.replace(BASE_URL, ""));
        let video_url = enter_iframe_wait_jwplayer(&driver, args, all_l, client, path).await?;
        return Ok(video_url);
    }

    // check next page 
    let n = driver.find_all(By::ClassName("animeps-next-page")).await?;

    // only one page
    if n.len() == 0 {
        all_l.extend(get_all_link_base_href(&driver, args).await?);
    }

    // iter over all page possible
    let page_return = next_page(&driver, args, &n).await?;
    all_l.extend(page_return);

    let video_url = enter_iframe_wait_jwplayer(&driver, args, all_l, client, path).await?;
    Ok(video_url)
}

async fn next_page(driver: &WebDriver, args: &Args, n: &Vec<WebElement>) -> Result<Vec<String>, Box<dyn Error>> {
    let mut all_links = vec![];
    while n.len() != 0 {
        all_links.extend(get_all_link_base_href(&driver, args).await?);
        let n = driver.find_all(By::ClassName("animeps-next-page")).await?;
        if !n
            .first()
            .expect("first")
            .attr("class")
            .await?
            .expect("euh")
            .contains("disabled")
        {
            info!("Next page");
            driver
                .execute(
                    r#"document.querySelector('.animeps-next-page').click();"#,
                    vec![],
                )
                .await?;
        } else {
            break;
        }
    }

    Ok(all_links)
}

pub async fn get_base_name_direct_url(driver: &WebDriver) -> String {
    let class = driver
        .find(By::XPath(
            r#"//*[@id="watch"]/div/div[4]/div[1]/div/div/h2/a"#,
        ))
        .await
        .expect("Can't get real name direct url");

    let path = class
        .inner_html()
        .await
        .expect("Can't get real name direct innerhtml");
    path
}

async fn get_all_link_base_href(driver: &WebDriver, args: &Args) -> Result<Vec<String>, Box<dyn Error>> {
    let mut url_found = vec![];
    let mut play_class = driver.find_all(By::ClassName("play")).await?;

    if play_class.len() == 0 {
        play_class = driver.find_all(By::ClassName("text-left")).await?;
    }

    for x in play_class {
        if let Some(url) = x.attr("href").await? {
            if args.debug {
                debug!("get_all_link_base_href: {url}")
            }
            url_found.push(url)
        }
    }
    Ok(url_found)
}

async fn enter_iframe_wait_jwplayer(driver: &WebDriver, args: &Args, all_l: Vec<String>, client: &Client, path: &AllPath) -> Result<(u16, u16), Box<dyn Error>> {
    let mut nb_found = 0u16;
    let mut nb_error = 0u16;

    for fuse_iframe in all_l {
        let url = format!("{BASE_URL}{fuse_iframe}");
        driver.handle.goto(&url).await?;

        let url = driver.handle.find(By::Id("un_episode")).await?;
        // force wait after iframe update jwplayer in html
        match url.handle.clone().enter_frame(0).await {
            Ok(_) => {
                loop {
                    match driver.handle.find(By::Id("main-player")).await {
                        Ok(e) => {
                            if let Ok(a) = e.attr("class").await {
                                if let Some(a) = a {
                                    if a.contains("jwplayer") {
                                        break;
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            continue;
                        }
                    }
                }
                let (found, error) =
                    find_and_get_m3u8(nb_found, nb_error, &driver, &path, &client, &args).await?;
                nb_found = found;
                nb_error = error;
            }
            Err(_) => {}
        }
        driver.handle.enter_parent_frame().await?;
    }
    // utils_data::kill_process()?;
    Ok((nb_found, nb_error))
}

async fn find_and_get_m3u8(mut nb_found: u16, mut nb_error: u16, driver: &WebDriver, path: &AllPath, client: &Client, args: &Args) -> Result<(u16, u16), Box<dyn Error>> {
    let name = utils_data::edit_for_windows_compatibility(
        &driver.title().await?.replace(" - Neko Sama", ""),
    );
    match driver
        .handle
        .execute(r#"return jwplayer().getPlaylistItem();"#, vec![])
        .await
    {
        Ok(script) => {
            info!("Get m3u8 for: {}", name);
            match script.json()["file"].as_str() {
                None => {
                    error!("can't exec js for {name}: {:?}", script)
                }
                Some(url) => {
                    download_and_save_m3u8(
                        url,
                        &name.trim().replace(":", "").replace(" ", "_"),
                        &path.tmp_dl,
                        &client,
                        args,
                    )
                        .await?;

                    nb_found += 1;
                }
            }
        }
        Err(e) => {
            error!("Can't get .m3u8 {name} (probably 404)\n{:?}", e);
            nb_error += 1;
        }
    }

    Ok((nb_found, nb_error))
}

async fn download_and_save_m3u8(url: &str, file_name: &str, tmp_dl: &PathBuf, client: &Client, args: &Args) -> Result<(), Box<dyn Error>> {
    match web::web_request(&client, &url).await {
        Ok(body) => match body.status() {
            StatusCode::OK => {
                let await_response = body.text().await?;
                let split = await_response.as_bytes();
                let parsed = m3u8_rs::parse_playlist_res(split);

                let good_url = test_resolution(parsed, &args, &client).await;

                let mut out =
                    File::create(format!("{}/{file_name}.m3u8", tmp_dl.to_str().unwrap()))
                        .expect("failed to create file");

                if args.debug {
                    debug!("create .m3u8 for {}", file_name);
                }

                io::copy(
                    &mut web::web_request(&client, &good_url)
                        .await?
                        .text()
                        .await?
                        .as_bytes(),
                    &mut out,
                )
                    .expect("Error copy");

                if args.debug {
                    debug!("write .m3u8 for {}", file_name);
                }
            }
            _ => error!("Error base url check: {:?}", body.status()),
        },
        Err(e) => {
            error!("fetch_url: {:?}", e)
        }
    }
    Ok(())
}

async fn test_resolution(parsed: Result<Playlist, nom::Err<nom::error::Error<&[u8]>>>, args: &Args, client: &Client) -> String {
    let mut _good_url = String::new();
    match parsed {
        Ok(Playlist::MasterPlaylist(pl)) => {
            if args.debug {
                debug!("MasterPlaylist {:#?}", pl);
            }
            for ele in pl.variants {
                let resolution = ele.resolution.expect("No resolution found").height;
                let test = web::web_request(&client, &ele.uri).await;
                match test {
                    Ok(code) => match code.status() {
                        StatusCode::OK => {
                            info!("Download as {}p", resolution);
                            _good_url = ele.uri;
                            if args.debug {
                                debug!("url .m3u8 {}", _good_url);
                            }
                            break;
                        }
                        _ => {
                            warn!("{}p not found, try next", resolution);
                        }
                    },
                    Err(e) => error!("m3u8 check resolution error {}", e),
                }
            }
        }
        Ok(Playlist::MediaPlaylist(_)) => {}
        Err(e) => println!("Error parse m3u8 : {:?}", e),
    }
    _good_url
}
