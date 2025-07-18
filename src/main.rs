
use core::fmt;
use std::{cmp::min, env, error::Error, fs::File, i32, time::Duration};
use colored::Colorize;
use dotenv::dotenv;
use env_logger::{Builder, Env};
use indicatif::{MultiProgress, ProgressBar, ProgressState, ProgressStyle};
use indicatif_log_bridge::LogWrapper;
use log::*;
use regex::Regex;
use reqwest::{header::{HeaderValue, USER_AGENT}, Client, Url};
use rusttype::{Font, Point};
use serde_json::Value;
use text_io::read;
use text_svg::Text;
use std::io::Write;

struct Song {
    artist: String,
    title: String,
    release_year: i32,
    youtube_year: i32,
    video_id: String,
    raw_title: String,
    detected_title: Option<String>,
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

        if progress_bar_pos < 25 {
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

        let (year, detected_title) = match get_music_braiz_year(&client, &artist, &title).await {
            Ok(year) => year,
            Err(_) => {
                warn!("{} {} - {}, {}", "Song not found.".red(), artist.red(), title.red(), "Skipping for now.".red());
                skipped.push(Song{artist, title, release_year: upload_date, youtube_year: upload_date, video_id: id, raw_title, detected_title: None});
                continue;
            }
        };

        let song = Song{artist, title, release_year: year, youtube_year: upload_date, video_id: id, raw_title, detected_title: Some(detected_title)};

        songs.push(song);

        if progress_bar_pos >= 27 {
            break;
        }// rmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmm
    }

    pb.finish_with_message("All data received.");
    multi.remove(&pb);

    info!("Revisiting songs that need manual intervention.");
    println!();

    let mut action_for_all = -1;

    for song in skipped.iter_mut() {
        loop {
            if action_for_all == -1 {
                println!();
                println!("{} {}", "Youtube title: ", song.raw_title.bright_green());
                println!("{} {} - {}", "Queried title: ", song.artist.bright_green(), song.title.bright_green());
                println!();
                println!("Actions:");
                println!("{} {}{}{}", "1".blue(), "Use YouTube upload date (".cyan(), song.youtube_year, ")".cyan());
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
                    match custom_query(&client, song).await {
                        Ok(_) => (),
                        Err(_) => continue,
                    }
                },
                _ => return Err("unknown input".into()),
            }
            info!("Using {} for {}", song.release_year.to_string().green(), song.raw_title.cyan());
            break;
        }
    }

    info!("All dates specified. Continuing with final rewiew...");

    songs.append(&mut skipped);
    
    let mut page = 0;
    let mut elements_per_page = 20;
    'outer: loop {
        let page_count = f32::ceil(songs.len() as f32 / elements_per_page as f32) as u32;

        let page_str = format!("Page {}/{}", page + 1, page_count);
        println!();
        println!("{}", page_str.green());
        let elements_displayed = draw_table(&songs, page, elements_per_page);
        println!();
        println!("{}", "Actions:");
        println!("{}", "Number to select element".cyan());
        println!("{}", "a/d to change page".cyan());
        println!("{}", "+/- to change number of elements per page".cyan());
        println!("{}", "y to finish".cyan());
        println!();
        print_input_arrow();
        let input: String = read!("{}\n");
        match input.parse::<u32>() {
            Ok(num) => {
                loop {
                    if num > min(elements_displayed, elements_per_page) || num < 1 {
                        continue 'outer;
                    }
                    let selected = songs.get_mut(((num - 1) + (page * elements_per_page)) as usize).unwrap();
                    println!("Selected:");
                    println!("{} {} - {}", "Title for card: ", selected.artist.bright_green(), selected.title.bright_green());
                    if let Some(title) = &selected.detected_title {
                        println!("{} {}", "Detected title: ", title.bright_green());
                    }
                    println!("{} {}", "Year:           ", selected.release_year.to_string().bright_green());
                    println!();
                    println!("Actions:");
                    println!("{} {}", "1".blue(), "New query".cyan());
                    println!("{} {}", "2".blue(), "Change artist".cyan());
                    println!("{} {}", "3".blue(), "Change title".cyan());
                    println!("{} {}", "4".blue(), "Change year".cyan());
                    println!("{} {}{}{}", "5".blue(), "Switch to YouTube year (".cyan(), selected.youtube_year.to_string().blue(), ")".cyan());
                    println!("{} {}", "6".blue(), "Return".cyan());
                    println!();
                    let action = input_num(1, 6);
                    match action {
                        1 => {
                            match custom_query(&client, selected).await {
                                Ok(_) => (),
                                Err(_) => continue,
                            }
                        },
                        2 => {
                            println!("    {}", selected.artist.blue());
                            print_input_arrow();
                            selected.artist = read!("{}\n");
                        },
                        3 => {
                            println!("    {}", selected.title.blue());
                            print_input_arrow();
                            selected.title = read!("{}\n");
                        },
                        4 => {
                            println!("    {}", selected.release_year.to_string().blue());
                            selected.release_year = input_num(i32::MIN, i32::MAX);
                        },
                        5 => {
                            selected.release_year = selected.youtube_year;
                            println!("Using {} for {}", selected.release_year.to_string().blue(), selected.raw_title.green());
                        },
                        6 => continue 'outer,
                        _ => return Err("unknown input".into()),
                    }
                    break;
                }
            },
            Err(_) => {
                match input.as_str() {
                    "a" => {
                        if page > 0 {
                            page -= 1;
                        }
                    },
                    "d" => {
                        if page < page_count - 1 {
                            page += 1;
                        }
                    },
                    "+" => {
                        elements_per_page += 10;
                    },
                    "-" => {
                        if elements_per_page > 10 {
                            elements_per_page -= 10;
                        }
                    },
                    "y" => break,
                    _ => continue,
                }
            }
        }
    }

    info!("Creating qr-codes...");

    let font_data = std::fs::read("./CalSans-SemiBold.ttf")
        .expect("Error reading font file");
    let font = Font::try_from_vec(font_data).expect("Failed to load font");

    let icon = std::fs::read("./Carnister.svg").expect("Error reading icon file").iter().fold(String::new(), |a, b| a + &(*b as char).to_string());
    let background_design = std::fs::read("./design0.svg").expect("Error reading design file").iter().fold(String::new(), |a, b| a + &(*b as char).to_string());

    let mut svg = Vec::new();

    // let link = format!("https://music.youtube.com/watch?v={}", songs[0].video_id);
    
    // let mut qr = qrcode_generator::to_svg_to_string(link, QrCodeEcc::Low, 50, None::<&str>).unwrap();
    // let qr = qr.split_off(qr.find("<path").unwrap());
    // let qr = qr.trim_end_matches("</svg>");

    svg.push("<svg viewBox=\"0 0 210 297\" version=\"1.1\" xmlns=\"http://www.w3.org/2000/svg\">".into());
    svg.push("<rect fill=\"#AAAAAA\" x=\"0\" y=\"0\" width=\"210\" height=\"297\"/>".into());

    svg.push("<svg x=\"0\" y=\"0\" width=\"65\" height=\"65\">".into());
    svg.push("<rect fill=\"#00FF00\" x=\"0\" y=\"0\" width=\"65\" height=\"65\"/>".into());
    svg.push(create_card_front_svg_component(&songs[0], &font, &icon, &background_design));
    svg.push("</svg>".into());

    svg.push("<svg x=\"65\" y=\"0\" width=\"65\" height=\"65\">".into());
    svg.push("<rect fill=\"#00FF00\" x=\"0\" y=\"0\" width=\"65\" height=\"65\"/>".into());
    svg.push(create_card_front_svg_component(&songs[0], &font, &icon, &background_design));
    svg.push("</svg>".into());

    svg.push("<svg x=\"130\" y=\"0\" width=\"65\" height=\"65\">".into());
    svg.push("<rect fill=\"#00FF00\" x=\"0\" y=\"0\" width=\"65\" height=\"65\"/>".into());
    svg.push(create_card_front_svg_component(&songs[0], &font, &icon, &background_design));
    svg.push("</svg>".into());

    svg.push("</svg>".into());

    let svg = svg.iter().fold(String::new(), |a, b| a + b + "\n");

    let mut output_file = File::create("./file_output.svg")?;
    writeln!(output_file, "{}", svg)?;

    Ok(())
}

fn create_card_front_svg_component(song: &Song, font: &Font, icon: &String, bg_design: &String) -> String {
    
    let mut svg = Vec::new();

    let year = Text::builder().size(30.0).start(Point {x: 0.0, y: 0.0}).build(font, &song.release_year.to_string());
    let year_x = (100.0 - year.bounding_box.width()) / 2.0;
    let year_y = (100.0 - year.bounding_box.height()) / 2.0 - year.bounding_box.height() / 5.0;

    let artist = Text::builder().size(5.0).start(Point {x: 0.0, y: 0.0}).build(font, &song.artist.to_string());
    let artist_x = (100.0 - artist.bounding_box.width()) / 2.0;
    let artist_y = 10.0;

    let title = Text::builder().size(5.0).start(Point {x: 0.0, y: 0.0}).build(font, &song.title.to_string());
    let title_x = (100.0 - title.bounding_box.width()) / 2.0;
    let title_y = 82.0;

    let bg = bg_design.clone().replace("=\"#05575d", "=\"#5d3705").replace("=\"#9c65ff", "=\"#ff6565");

    svg.push("<svg viewBox=\"0 0 100 100\">".into());

    svg.push(bg);

    svg.push(format!("<svg x=\"{}\" y=\"{}\">", year_x, year_y));
    svg.push(year.path.to_string());
    svg.push("</svg>".into());

    svg.push(format!("<svg x=\"{}\" y=\"{}\">", artist_x, artist_y));
    svg.push(artist.path.to_string());
    svg.push("</svg>".into());

    svg.push(format!("<svg x=\"{}\" y=\"{}\">", title_x, title_y));
    svg.push(title.path.to_string());
    svg.push("</svg>".into());

    svg.push("<svg x=\"3\" y=\"3\" width=\"10\" height=\"10\" viewBox=\"0 0 100 100\">".into());
    svg.push(icon.clone());
    svg.push("</svg>".into());

    svg.push("</svg>".into());

    svg.iter().fold(String::new(), |a, b| a + b + "\n")
}

async fn custom_query(client: &Client, song: &mut Song) -> Result<(), Box<dyn Error>> {
    println!("Artist:");
    print_input_arrow();
    let custom_query_artist: String = read!("{}\n");
    println!("Title:");
    print_input_arrow();
    let custom_query_title: String = read!("{}\n");
    match get_music_braiz_year(&client, &custom_query_artist, &custom_query_title).await {
        Ok((year, detected_title)) => {
            song.release_year = year;
            song.detected_title = Some(detected_title);
        },
        Err(_) => {
            info!("{}", "Song not found".red());
            return Err("Song not found".into());
        }
    }
    Ok(())
}

fn draw_table(elements: &Vec<Song>, page: u32, elements_per_page: u32) -> u32 {

    let longest_artist = elements.iter().map(|s| s.artist.len()).max().unwrap_or(0) as u32;
    let longest_title = elements.iter().map(|s| s.title.len()).max().unwrap_or(0) as u32;
    let longest_detected_title = elements.iter().map(|s| s.detected_title.clone().unwrap_or("".to_string()).len()).max().unwrap_or(0) as u32;
    let longest_year = elements.iter().map(|s| s.release_year.to_string().len()).max().unwrap_or(0) as u32;

    let mut displayed_songs: Vec<Option<&Song>> = Vec::new();
    for i in 0..elements_per_page {
        displayed_songs.push(elements.get((i + (page * elements_per_page)) as usize));
    }

    let displayed_songs_count = displayed_songs.iter().filter(|s| s.is_some()).count() as u32;

    const TABLE_R: u8 = 100;
    const TABLE_G: u8 = TABLE_R;
    const TABLE_B: u8 = TABLE_R;

    print!("{}", "┌────┬".truecolor(TABLE_R, TABLE_G, TABLE_B));
    for _ in 0..longest_artist + 2 {
        print!("{}", "─".truecolor(TABLE_R, TABLE_G, TABLE_B));
    }
    print!("{}", "┬".truecolor(TABLE_R, TABLE_G, TABLE_B));
    for _ in 0..longest_title + 2 {
        print!("{}", "─".truecolor(TABLE_R, TABLE_G, TABLE_B));
    }
    print!("{}", "┬".truecolor(TABLE_R, TABLE_G, TABLE_B));
    for _ in 0..longest_detected_title + 2 {
        print!("{}", "─".truecolor(TABLE_R, TABLE_G, TABLE_B));
    }
    print!("{}", "┬".truecolor(TABLE_R, TABLE_G, TABLE_B));
    for _ in 0..longest_year + 2 {
        print!("{}", "─".truecolor(TABLE_R, TABLE_G, TABLE_B));
    }
    println!("{}", "┐".truecolor(TABLE_R, TABLE_G, TABLE_B));
    print!("{}", "│ ## │ ".truecolor(TABLE_R, TABLE_G, TABLE_B));
    print!("{}", "Artist".to_string().green());
    fillup_spaces("Artist".to_string(), longest_artist + 1);
    print!("{}", "│ ".truecolor(TABLE_R, TABLE_G, TABLE_B));
    print!("{}", "Title".to_string().green());
    fillup_spaces("Title".to_string(), longest_title + 1);
    print!("{}", "│ ".truecolor(TABLE_R, TABLE_G, TABLE_B));
    print!("{}", "Detected Title".to_string().green());
    fillup_spaces("Detected Title".to_string(), longest_detected_title + 1);
    print!("{}", "│ ".truecolor(TABLE_R, TABLE_G, TABLE_B));
    print!("{}", "Year".to_string().green());
    fillup_spaces("Year".to_string(), longest_year + 1);
    println!("{}", "│ ".truecolor(TABLE_R, TABLE_G, TABLE_B));

    print!("{}", "├────┼".truecolor(TABLE_R, TABLE_G, TABLE_B));
    for _ in 0..longest_artist + 2 {
        print!("{}", "─".truecolor(TABLE_R, TABLE_G, TABLE_B));
    }
    print!("{}", "┼".truecolor(TABLE_R, TABLE_G, TABLE_B));
    for _ in 0..longest_title + 2 {
        print!("{}", "─".truecolor(TABLE_R, TABLE_G, TABLE_B));
    }
    print!("{}", "┼".truecolor(TABLE_R, TABLE_G, TABLE_B));
    for _ in 0..longest_detected_title + 2 {
        print!("{}", "─".truecolor(TABLE_R, TABLE_G, TABLE_B));
    }
    print!("{}", "┼".truecolor(TABLE_R, TABLE_G, TABLE_B));
    for _ in 0..longest_year + 2 {
        print!("{}", "─".truecolor(TABLE_R, TABLE_G, TABLE_B));
    }
    println!("{}", "┤".truecolor(TABLE_R, TABLE_G, TABLE_B));

    let mut num = 1;
    for song in displayed_songs {
        
        let artist;
        let title;
        let detected_title;
        let year;
        let base_detected_title;
        match song {
            Some(song) => {
                artist = song.artist.clone();
                title = song.title.clone();

                detected_title = match song.detected_title.clone() {
                    Some(mut raw_detected_title) => {

                        base_detected_title = raw_detected_title.clone();
                
                        let detected_title_copy = raw_detected_title.clone();
                        let (artist_split, title_split) = detected_title_copy.split_once(" - ").unwrap();
                        let artists: Vec<&str> = artist_split.split(", ").collect();

                        for det_artist in artists {
                            match raw_detected_title.split_once(det_artist) {
                                Some((left, right)) => {
                                    if artist.to_lowercase().contains(det_artist.to_lowercase().as_str()) {
                                        raw_detected_title = left.to_string() + &det_artist.green().to_string() + right;
                                    }
                                },
                                None => (),
                            }
                        }
                        match raw_detected_title.split_once(title_split) {
                            Some((left, right)) => {
                                if title.to_lowercase().contains(title_split.to_lowercase().as_str()) {
                                    raw_detected_title = left.to_string() + &title_split.green().to_string() + right;
                                }
                            },
                            None => (),
                        }

                        raw_detected_title.blue().to_string()
                    },
                    None => {
                        base_detected_title = String::new();
                        String::new()
                    }
                };

                year = song.release_year.to_string();
            },
            None => {
                artist = String::new();
                title = String::new();
                detected_title = String::new();
                year = String::new();
                base_detected_title = String::new();
            },
        };
        let num_str = (if num <= 9 {" ".to_string()} else {"".to_string()}) + &num.to_string();
        print!("{}", "│ ".truecolor(TABLE_R, TABLE_G, TABLE_B));
        print!("{}{}", num_str.blue(), " │ ".truecolor(TABLE_R, TABLE_G, TABLE_B));
        print!("{}", artist.green());
        fillup_spaces(artist, longest_artist + 1);
        print!("{}", "│ ".truecolor(TABLE_R, TABLE_G, TABLE_B));
        print!("{}", title.green());
        fillup_spaces(title, longest_title + 1);
        print!("{}", "│ ".truecolor(TABLE_R, TABLE_G, TABLE_B));
        print!("{}", detected_title.green());
        fillup_spaces(base_detected_title, longest_detected_title + 1);
        print!("{}", "│ ".truecolor(TABLE_R, TABLE_G, TABLE_B));
        print!("{}", year.green());
        fillup_spaces(year, longest_year + 1);
        println!("{}", "│ ".truecolor(TABLE_R, TABLE_G, TABLE_B));

        num += 1;
    }

    print!("{}", "└────┴".truecolor(TABLE_R, TABLE_G, TABLE_B));
    for _ in 0..longest_artist + 2 {
        print!("{}", "─".truecolor(TABLE_R, TABLE_G, TABLE_B));
    }
    print!("{}", "┴".truecolor(TABLE_R, TABLE_G, TABLE_B));
    for _ in 0..longest_title + 2 {
        print!("{}", "─".truecolor(TABLE_R, TABLE_G, TABLE_B));
    }
    print!("{}", "┴".truecolor(TABLE_R, TABLE_G, TABLE_B));
    for _ in 0..longest_detected_title + 2 {
        print!("{}", "─".truecolor(TABLE_R, TABLE_G, TABLE_B));
    }
    print!("{}", "┴".truecolor(TABLE_R, TABLE_G, TABLE_B));
    for _ in 0..longest_year + 2 {
        print!("{}", "─".truecolor(TABLE_R, TABLE_G, TABLE_B));
    }
    println!("{}", "┘".truecolor(TABLE_R, TABLE_G, TABLE_B));

    return displayed_songs_count;

}

fn fillup_spaces(string: String, length: u32) {
    for _ in (string.len() as u32)..length {
        print!(" ");
    }
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

async fn get_music_braiz_year(client: &Client, artist: &str, title: &str) -> Result<(i32, String), Box<dyn std::error::Error>> {

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

    let mut date = json["recordings"][0]["first-release-date"].as_str().unwrap_or("").to_string();

    let first_dash = match date.find("-") {
        Some(index) => index,
        None => return Err(format!("Date Parsing Error: {}, {}", date, url).into())
    };

    let artists: Vec<String> = json["recordings"][0]["artist-credit"].as_array().unwrap().iter().map(|val| val["name"].as_str().unwrap_or("").to_string()).collect();
    let detected_title = artists.iter().fold("".to_string(), |a, b| a + ", " + b).split_off(2) + " - " + json["recordings"][0]["title"].as_str().unwrap_or("");
    info!("{} {}", "Found:".green(), detected_title);

    date.truncate(first_dash);

    Ok((date.parse::<i32>().unwrap(), detected_title))
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