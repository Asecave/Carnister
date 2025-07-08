
use core::fmt;
use std::{env, error::Error, fmt::Write, time::Duration};
use colored::Colorize;
use dotenv::dotenv;
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use regex::Regex;
use reqwest::{header::{HeaderValue, USER_AGENT}, Client, Url};
use serde_json::Value;

struct Song {
    artist: String,
    title: String,
    release_year: i32,
    video_id: String,
}

impl fmt::Display for Song {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}, {}, {}, {}", self.artist, self.title, self.release_year, self.video_id)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv().ok();

    let api_key = env::var("YOUTUBE_API_KEY").expect("Specify a Youtube API key");
    let playlist_id = "PLP9X6Hp3ZLpOsDk3AudxA5FueNmcrQTLr";

    println!("Fetching videos from playlist...");

    let videos = fetch_videos(&api_key, playlist_id).await.expect("Error while fetching videos");

    let mut songs: Vec<Song> = Vec::new();
    let mut skipped: Vec<Song> = Vec::new();
    let client = Client::new();
    let timeout = 1050;

    let pb = ProgressBar::new(videos.len() as u64);
    pb.set_style(ProgressStyle::with_template("[{elapsed_precise}] [{wide_bar:.cyan/black}] {pos:>7}/{len:7} ({eta})")
        .unwrap()
        .with_key("eta", |state: &ProgressState, w: &mut dyn Write| write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap())
        .progress_chars("=>-"));

    println!("Setting request delay to {}ms to not get rate limited by MusicBrainz (they accept around 1 request per second)", timeout);
    println!("Receiving data...");

    let mut progress_bar_pos = 0;

    for video in videos {

        pb.set_position(progress_bar_pos);
        progress_bar_pos += 1;

        let id = video["snippet"]["resourceId"]["videoId"].to_string().trim_matches('\"').to_string();
        let raw_title = video["snippet"]["title"].to_string().trim_matches('\"').to_string();
        let upload_channel = video["snippet"]["videoOwnerChannelTitle"].to_string().trim_matches('\"').to_string();
        let raw_upload_date = video["snippet"]["publishedAt"].to_string().trim_matches('\"').to_string();

        
        let title;
        let artist;
        let upload_date;

        let mut tmp_upload_date = raw_upload_date.clone();
        tmp_upload_date.truncate(raw_upload_date.find("-").unwrap());
        upload_date = tmp_upload_date.parse::<i32>().unwrap();

        if raw_title.find(" - ") == None {
            artist = upload_channel.replace(" - Topic", "");
            title = clean_title(&raw_title);
        } else {
            let split_title: Vec<&str> = raw_title.split(" - ").collect();
            artist = split_title[0].to_string();
            title = clean_title(&split_title[1].to_string());
        }

        tokio::time::sleep(Duration::from_millis(timeout)).await;

        let year = match get_music_braiz_year(&client, &artist, &title).await {
            Ok(year) => year,
            Err(_) => {
                println!("{} {} - {}, {}", "Song not found.".red(), artist.red(), title.red(), "Skipping for now.".red());
                skipped.push(Song{artist, title, release_year: upload_date, video_id: id});
                continue;
            }
        };

        let song = Song{artist, title, release_year: year, video_id: id};

        songs.push(song);
    }

    pb.finish_with_message("All data received.");

    Ok(())
}

async fn fetch_videos(api_key: &str, playlist_id: &str) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let client = Client::new();
    let mut videos = Vec::new();
    let mut page_token = String::new();

    loop {
        
        let url = format!(
            "https://youtube.googleapis.com/youtube/v3/playlistItems?part=snippet&maxResults=50&playlistId={}&pageToken={}&key={}",
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

    println!("{} {}", "Getting".truecolor(100, 100, 100), &url.truecolor(100, 100, 100));

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
    println!("{} {} - {}", "Found:".green(), artists.iter().fold("".to_string(), |a, b| a + ", " + b).split_off(2), json["recordings"][0]["title"]);

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

fn clean_title(input: &String) -> String {
    let bracket_re = Regex::new(r"\[.*?\]").unwrap();
    let remix_re = Regex::new(r"\([^)]*\)").unwrap();
    let other_re = Regex::new(r"\|.*").unwrap();
    let temp = bracket_re.replace_all(input.as_str(), "").to_string();
    let temp = remix_re
        .replace_all(&temp, |caps: &regex::Captures| {
            let content = &caps[0].to_lowercase();
            if content.contains("remix") || content.contains("edit") {
                caps[0].to_string()
            } else {
                "".to_string()
            }
        })
        .to_string();
    let temp = other_re.replace_all(&temp, "").to_string();
    temp.trim().to_string()
}