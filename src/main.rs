
use core::fmt;
use std::{env, error::Error, time::Duration};
use colored::Colorize;
use dotenv::dotenv;
use env_logger::{Builder, Env};
use indicatif::{MultiProgress, ProgressBar, ProgressState, ProgressStyle};
use indicatif_log_bridge::LogWrapper;
use log::*;
use regex::Regex;
use reqwest::{header::{HeaderValue, USER_AGENT}, Client, Url};
use serde_json::Value;
use text_io::read;
use std::io::Write;

struct Song {
    artist: String,
    title: String,
    release_year: i32,
    video_id: String,
    raw_title: String,
}

impl fmt::Display for Song {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}, {}, {}, {}, {}", self.artist, self.title, self.release_year, self.video_id, self.raw_title)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv().ok();

    let logger = 
        Builder::from_env(Env::default().default_filter_or("info"))
        .format(|buf, record| {

            let level = match record.level() {
                log::Level::Info  => "INFO ".green(),
                log::Level::Warn  => "WARN ".yellow(),
                log::Level::Error => "ERROR".red(),
                log::Level::Debug => "DEBUG".blue(),
                log::Level::Trace => "TRACE".blue(),
            };

            let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
            writeln!(buf, "{}{} {}{} {}", "[".truecolor(150, 150, 150), timestamp.truecolor(150, 150, 150), level, "]".truecolor(150, 150, 150), record.args())
        })
        .build();
    let level = logger.filter();
    let multi = MultiProgress::new();

    LogWrapper::new(multi.clone(), logger)
        .try_init()
        .unwrap();
    log::set_max_level(level);

    let api_key = env::var("YOUTUBE_API_KEY").expect("Specify a Youtube API key");
    let playlist_id = "PLP9X6Hp3ZLpOsDk3AudxA5FueNmcrQTLr";

    info!("Fetching videos from playlist...");

    let videos = fetch_videos(&api_key, playlist_id).await.expect("Error while fetching videos");

    let mut songs: Vec<Song> = Vec::new();
    let mut skipped: Vec<Song> = Vec::new();
    let client = Client::new();
    let timeout = 1050;

    info!("Setting request delay to {}ms to not get rate limited by MusicBrainz (they accept around 1 request per second)", timeout);
    info!("Receiving data...");

    let pb = multi.add(ProgressBar::new(videos.len() as u64));
    pb.set_style(ProgressStyle::with_template("[{elapsed_precise}] [{wide_bar:.cyan/black}] {pos:>7}/{len:7} ({eta})")
        .unwrap()
        .with_key("eta", |state: &ProgressState, w: &mut dyn fmt::Write| write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap())
        .progress_chars("=>-"));

    let mut progress_bar_pos = 0;

    for video in videos {
        
        pb.set_position(progress_bar_pos);
        progress_bar_pos += 1;

        if progress_bar_pos < 17 {
            continue;
        }
        
        let id = video["contentDetails"]["videoId"].to_string().trim_matches('\"').to_string();
        let raw_title = video["snippet"]["title"].to_string().trim_matches('\"').to_string();
        let upload_channel = video["snippet"]["videoOwnerChannelTitle"].to_string().trim_matches('\"').to_string();
        let raw_upload_date = video["contentDetails"]["videoPublishedAt"].to_string().trim_matches('\"').to_string();

        
        let title;
        let artist;
        let upload_date;

        let mut tmp_upload_date = raw_upload_date.clone();
        tmp_upload_date.truncate(raw_upload_date.find("-").unwrap());
        upload_date = tmp_upload_date.parse::<i32>().unwrap();

        if raw_title.find(" - ") == None {
            artist = clean_artist(&upload_channel.replace(" - Topic", ""));
            title = clean_title(&raw_title);
        } else {
            let split_title: Vec<&str> = raw_title.split(" - ").collect();
            artist = clean_artist(&split_title[0].to_string());
            title = clean_title(&split_title[1].to_string());
        }

        tokio::time::sleep(Duration::from_millis(timeout)).await;

        let year = match get_music_braiz_year(&client, &artist, &title).await {
            Ok(year) => year,
            Err(_) => {
                warn!("{} {} - {}, {}", "Song not found.".red(), artist.red(), title.red(), "Skipping for now.".red());
                skipped.push(Song{artist, title, release_year: upload_date, video_id: id, raw_title});
                continue;
            }
        };

        let song = Song{artist, title, release_year: year, video_id: id, raw_title};

        songs.push(song);

        if progress_bar_pos >= 22 {
            break;
        }// rmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmm
    }

    pb.finish_with_message("All data received.");
    multi.remove(&pb);

    info!("Revisiting songs that need manual intervention.");
    println!();

    let mut action_for_all = -1;

    for mut song in skipped {
        loop {
            if action_for_all == -1 {
                println!();
                println!("{} {}", "Youtube title: ", song.raw_title.bright_green());
                println!("{} {} - {}", "Queried title: ", song.artist.bright_green(), song.title.bright_green());
                println!();
                println!("Actions:");
                println!("{} {}", "1".blue(), "Use YouTube upload date".cyan());
                println!("{} {}", "2".blue(), "Manually set release year".cyan());
                println!("{} {}", "3".blue(), "Edit song name for database query".cyan());
                println!("{} {}", "4".blue(), "Use YouTube upload date for all remaining".cyan());
                println!("{} {}", "5".blue(), "Manually set release year for all remaining".cyan());
                println!();
                println!("Enter number:");
            }
            let mut input = 0;
            if action_for_all == -1 {
                input = input_num(1, 5);
                if input == 4 {action_for_all = 1}
                if input == 5 {action_for_all = 2}
            }
            if action_for_all != -1 {
                input = action_for_all;
            }

            match input {
                1 => (),
                2 => {
                    println!("Enter year for {}:", song.raw_title.bright_green());
                    song.release_year = input_num(i32::MIN, i32::MAX);
                },
                3 => {
                    println!("Artist:");
                    print_input_arrow();
                    let custom_query_artist: String = read!("{}\n");
                    println!("Title:");
                    print_input_arrow();
                    let custom_query_title: String = read!("{}\n");
                    match get_music_braiz_year(&client, &custom_query_artist, &custom_query_title).await {
                        Ok(year) => {
                            song.release_year = year;
                        },
                        Err(_) => {
                            info!("{}", "Song not found".red());
                            continue;
                        }
                    }

                },
                _ => return Err("unknown input".into()),
            }
            info!("Using {} for {}", song.release_year.to_string().green(), song.raw_title.cyan());
            break;
        }
    }

    Ok(())
}

fn print_input_arrow() {
    print!("{}", "==> ".green());
}

fn input_num(range_min: i32, range_max: i32) -> i32 {
    loop {
        print_input_arrow();
        let raw_input: String = read!("{}\n");
        match raw_input.parse::<i32>() {
            Ok(i) => {
                if i >= range_min && i <= range_max {
                    return i;
                }
            },
            Err(_) => ()
        };
    }
}

async fn fetch_videos(api_key: &str, playlist_id: &str) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let client = Client::new();
    let mut videos = Vec::new();
    let mut page_token = String::new();

    loop {
        
        let url = format!(
            "https://youtube.googleapis.com/youtube/v3/playlistItems?part=snippet&part=contentDetails&maxResults=50&playlistId={}&pageToken={}&key={}",
            playlist_id, page_token, api_key
        );

        let json = receive_json(&client, &url).await.unwrap();

        if let Some(items) = json["items"].as_array() {
            videos.extend(items.clone());
        }

        if let Some(next_page_token) = json["nextPageToken"].as_str() {
            page_token = next_page_token.to_string();
        } else {
            break;
        }
    }

    Ok(videos)
}

async fn get_music_braiz_year(client: &Client, artist: &str, title: &str) -> Result<i32, Box<dyn std::error::Error>> {

    let url = format!("https://musicbrainz.org/ws/2/recording?query=recording:\"{}\" AND artist:\"{}\"&fmt=json", &title, &artist);

    info!("{} {} {} {}", "Getting".truecolor(100, 100, 100), artist.truecolor(100, 100, 100), "-".truecolor(100, 100, 100), title.truecolor(100, 100, 100));

    let json = receive_json(client, &url).await.unwrap();
    let result_count = json["recordings"].as_array().unwrap().len();

    if result_count == 0 {
        return Err(format!("Song not found. Url: {}", url).into())
    }

    if json["recordings"][0]["first-release-date"].is_null() {
        return Err(format!("No date found. Url: {}", url).into());
    }

    let mut date = json["recordings"][0]["first-release-date"].to_string().trim_matches('\"').to_string();

    let first_dash = match date.find("-") {
        Some(index) => index,
        None => return Err(format!("Date Parsing Error: {}, {}", date, url).into())
    };

    let artists: Vec<String> = json["recordings"][0]["artist-credit"].as_array().unwrap().iter().map(|val| val["name"].to_string()).collect();
    info!("{} {} - {}", "Found:".green(), artists.iter().fold("".to_string(), |a, b| a + ", " + b).split_off(2), json["recordings"][0]["title"]);

    date.truncate(first_dash);

    Ok(date.parse::<i32>().unwrap())
}

async fn receive_json(client: &Client, url: &str) -> Result<Value, Box<dyn std::error::Error>> {

    let url_url = Url::parse(&url).expect(format!("Non valid url: {}", &url).as_str());

    let header = HeaderValue::from_str("Carnister/1.0 (https://github.com/Asecave/Carnister/issues)").unwrap();
    let response = client.get(url).header(USER_AGENT, header).send().await?;

    if !response.status().is_success() {
        println!("{} request failed with status: {}", url_url.host_str().unwrap_or("json"), response.status());
        println!("Response body: {}", response.text().await?);
        return Err(format!("{} request failed", url_url.host_str().unwrap_or("json")).into());
    }
    
    let json: Value = response.json().await?;
    
    if let Some(error) = json.get("error"){
        print!("{} returned an error: {:?}", url_url.host_str().unwrap_or("json"), error);
        return Err(format!("{} returned an error", url_url.host_str().unwrap_or("json")).into());
    }

    Ok(json)
}

fn clean_artist(input: &String) -> String {
    let bracket_re = Regex::new(r"\[.*?\]").unwrap();
    let mut temp = bracket_re.replace_all(&input, "").trim().to_string();
    if let Some(index) = temp.find("-").or(temp.find("|")) {
        temp = temp.split_off(index);
    }
    temp.trim().to_string()
}

fn clean_title(input: &String) -> String {
    let bracket_re = Regex::new(r"\[.*?\]").unwrap();
    let remix_re = Regex::new(r"\([^)]*\)").unwrap();
    let other_re = Regex::new(r"\|.*").unwrap();
    let temp = bracket_re.replace_all(input.as_str(), "").to_string();
    let temp = remix_re
        .replace_all(&temp, |caps: &regex::Captures| {
            let content = &caps[0].to_lowercase();
            if content.contains("remix") || content.contains("edit") || content.contains("vip") {
                caps[0].to_string()
            } else {
                "".to_string()
            }
        })
        .to_string();
    let temp = other_re.replace_all(&temp, "").to_string();
    temp.trim().to_string()
}