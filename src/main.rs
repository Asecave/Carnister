
use std::{env, error::Error};
use dotenv::dotenv;
use reqwest::Client;
use serde_json::Value;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv().ok();

    let api_key = env::var("YOUTUBE_API_KEY").expect("Specify a Youtube API key");
    let playlist_id = "PLP9X6Hp3ZLpOsDk3AudxA5FueNmcrQTLr";

    let videos = fetch_videos(&api_key, playlist_id).await.expect("Error while fetching videos");

    for video in videos {
        let id = &video["snippet"]["resourceId"]["videoId"];
        let title = &video["snippet"]["title"];
        let upload_channel = &video["snippet"]["videoOwnerChannelTitle"];
        let upload_date = &video["snippet"]["publishedAt"];
        println!("{}, {}, {}, {}", id, title, upload_channel, upload_date);
    }

    Ok(())
}

async fn fetch_videos(api_key: &str, playlist_id: &str) -> Result<Vec<Value>, Box<dyn std::error::Error>>{
    let client = Client::new();
    let mut videos = Vec::new();
    let mut page_token = String::new();

    loop {
        let url = format!(
            "https://youtube.googleapis.com/youtube/v3/playlistItems?part=snippet&maxResults=50&playlistId={}&pageToken={}&key={}",
            playlist_id, page_token, api_key
        );

        let response = client.get(&url).send().await?;

        if !response.status().is_success() {
            println!("API request failed with status: {}", response.status());
            println!("Response body: {}", response.text().await?);
            return Err("API request failed".into());
        }

        let json: Value = response.json().await?;

        if let Some(error) = json.get("error"){
            print!("API returned an error: {:?}", error);
            return Err("API returned an error".into());
        }

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
