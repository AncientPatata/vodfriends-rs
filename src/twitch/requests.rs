use crate::twitch::download_options::DownloadOptions;
use crate::twitch::gql::GQL_GET_ACCESS_TOKEN_QUERY;
use crate::twitch::utils::TwitchClientError;

use chrono::{NaiveDateTime, Utc};
use reqwest;
use serde_json;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use tokio;

use super::download_options;

const CLIENT_ID: &str = "kd1unb4b3q4t58fwlpcbzcbnm76a8fp";
const TWITCH_GQL_API: &str = "https://gql.twitch.tv/gql";

pub struct VideoInfo {
    pub video_id: String,
}
type ProgressCallback = fn(VideoInfo, f64);

pub struct TwitchClient {
    pub client: reqwest::Client,
    temp_folder: String,
    download_progress: ProgressCallback,
}

impl TwitchClient {
    pub fn new() -> TwitchClient {
        TwitchClient {
            client: reqwest::Client::new(),
            temp_folder: "/twitch_download_tmp/".to_string(),
            download_progress: |_, _| {},
        }
    }

    pub fn get_temp_folder(self) -> String {
        self.temp_folder
    }

    pub fn set_temp_folder(&mut self, new_temp_folder: String) {
        self.temp_folder = new_temp_folder;
    }

    pub fn set_progress_callback(&mut self, progress_callback: ProgressCallback) {
        self.download_progress = progress_callback;
    }

    async fn get_access_token(&self, video_id: String) -> Result<(String, String), reqwest::Error> {
        let request_json = json!({
            "operationName":"PlaybackAccessToken_Template",
            "query": GQL_GET_ACCESS_TOKEN_QUERY,
            "variables": {
                "isLive": false,
                "login": "",
                "isVod": true,
                "vodID": video_id,
                "playerType": "embed"
            }
        });
        let response = self
            .client
            .post(TWITCH_GQL_API)
            .json(&request_json)
            .header("Client-ID", CLIENT_ID)
            .send()
            .await?;
        //let response = response.error_for_status()?;
        let json: serde_json::Value = response.json().await?;

        let access_token = json["data"]["videoPlaybackAccessToken"]["value"]
            .as_str()
            .unwrap();

        let access_token_signature = json["data"]["videoPlaybackAccessToken"]["signature"]
            .as_str()
            .unwrap();

        Ok((access_token.to_owned(), access_token_signature.to_owned()))
    }

    pub async fn get_video_playlists(
        &self,
        video_id: String,
        access_token: String,
        signature: String,
    ) -> Result<Vec<String>, reqwest::Error> {
        let uri = format!(
            "http://usher.ttvnw.net/vod/{}?nauth={}&nauthsig={}&allow_source=true&player=twitchweb",
            video_id, access_token, signature
        );
        let response = self
            .client
            .get(uri)
            .header("Client-ID", CLIENT_ID)
            .send()
            .await?;
        let response = response.error_for_status()?;
        let playlist = response.text().await?;
        let playlists: Vec<String> = playlist.split("\n").map(|x| x.to_string()).collect();
        Ok(playlists)
    }

    pub fn calculate_vod_age(video_chunks: &[&str]) -> Result<f64, TwitchClientError> {
        let id3_equiv_tdtg = video_chunks
            .iter()
            .find(|x| x.starts_with("#ID3-EQUIV-TDTG:"))
            .unwrap()
            .to_string();

        let timestamp = NaiveDateTime::parse_from_str(
            id3_equiv_tdtg
                .strip_prefix("#ID3-EQUIV-TDTG:")
                .ok_or(TwitchClientError::new(
                    "Couldn't strip prefix when trying to calculate VOD age",
                ))
                .unwrap()
                .trim(),
            "%Y-%m-%dT%H:%M:%S",
        )
        .map_err(|_| {
            TwitchClientError::new("Couldn't get timespamp when trying to calculate VOD age")
        })?;
        let current_time = Utc::now();
        let duration = current_time.signed_duration_since(timestamp.and_utc());
        let total_hours = duration.num_hours() as f64;
        Ok(total_hours)
    }

    pub async fn download_vod(
        &self,
        playlist_url: String,
        download_options: DownloadOptions,
    ) -> Result<(), TwitchClientError> {
        let res = self
            .client
            .get(&playlist_url)
            .send()
            .await
            .map_err(|_| TwitchClientError::new("Couldn't get video chunks ?"))?
            .text()
            .await
            .map_err(|_| TwitchClientError::new("Couldn't get video chunks"))?;
        let video_chunks: Vec<&str> = res.split("\n").collect();
        let vod_age: f64 = TwitchClient::calculate_vod_age(&video_chunks).unwrap();
        let unmute = vod_age < 24.0;
        // Parsing video chunks :
        let mut video_list = HashMap::<String, f64>::new(); //What's this ? ???
        for i in 0..video_chunks.len() {
            if video_chunks[i].starts_with("#EXTINF") {
                if video_chunks[i + 1].starts_with("#EXT-X-BYTERANGE") {
                    if video_list.contains_key(video_chunks[i + 2]) {
                        // What the fuck is this ? Someone explain.
                        let mut pair = video_list
                            .iter()
                            .filter(|(x, _)| x.to_string() == video_chunks[i + 2])
                            .collect::<Vec<(&String, &f64)>>()[0];
                        pair = (
                            pair.0,
                            &(pair.1
                                + video_chunks[i][8..]
                                    .trim_end_matches(",")
                                    .parse::<f64>()
                                    .unwrap()),
                        )
                    } else {
                        video_list.insert(
                            video_chunks[i + 2].to_string(),
                            video_chunks[i][8..]
                                .trim_end_matches(",")
                                .parse::<f64>()
                                .unwrap(),
                        );
                    }
                } else {
                    video_list.insert(
                        video_chunks[i + 1].to_string(),
                        video_chunks[i][8..]
                            .trim_end_matches(",")
                            .parse::<f64>()
                            .unwrap(),
                    );
                }
            }
        }
        //println!("Number of Video Parts : {}", video_list.len());
        fs::create_dir_all(self.temp_folder.to_string()).map_err(|e| {
            TwitchClientError::new(&format!(
                "Failed to create download directory, encountered error : {}",
                e
            ))
        })?;
        let total_video_parts = video_list.len();
        let mut curr_video_parts = 0;
        // Start downloading parts :
        for part in video_list {
            let (partname, _) = part;
            if (unmute && partname.contains("-muted")) {
                self.download_video_part(
                    &(playlist_url[0..playlist_url.rfind("/").unwrap()].to_string()),
                    &partname.replace("-muted", ""),
                )
                .await?;
            } else {
                self.download_video_part(
                    &(playlist_url[0..playlist_url.rfind("/").unwrap()].to_string()),
                    &partname,
                )
                .await?;
            }
            curr_video_parts += 1;
            (self.download_progress)(
                VideoInfo {
                    video_id: download_options.video_id.to_string(),
                },
                curr_video_parts as f64 / total_video_parts as f64,
            );
        }

        self.combine_video_parts(download_options).await?;

        Ok(())
    }

    async fn combine_video_parts(
        &self,
        download_options: DownloadOptions,
    ) -> Result<(), TwitchClientError> {
        let files = fs::read_dir(&self.temp_folder)
            .map_err(|_| TwitchClientError::new("Failed to read directory."))?;
        let download_dir = Path::new(&download_options.filepath);
        fs::create_dir_all(download_dir).map_err(|e| {
            TwitchClientError::new(&format!(
                "Failed to create download directory : {}, encountered error : {}",
                download_dir.to_str().unwrap(),
                e
            ))
        })?;
        let mut output_file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&download_dir.join(download_options.filename))
            .map_err(|e| {
                TwitchClientError::new(&format!(
                    "Failed to create output file, encountered error : {}",
                    e
                ))
            })?;

        for file in files {
            let file = file.map_err(|_| TwitchClientError::new("Failed to read directory."))?;
            let file_path = file.path();

            if file_path.is_file()
                && file_path.extension().is_some()
                && file_path.extension().unwrap() == "ts"
            {
                let mut input_file = fs::File::open(&file_path)
                    .map_err(|_| TwitchClientError::new("Failed to open input file."))?;

                // Read input file and write its contents to the output file
                let mut buffer = vec![0; 1024]; // Buffer size for reading
                loop {
                    let bytes_read = input_file
                        .read(&mut buffer)
                        .map_err(|_| TwitchClientError::new("Failed to read input file."))?;
                    if bytes_read == 0 {
                        break;
                    }
                    output_file
                        .write_all(&buffer[..bytes_read])
                        .map_err(|_| TwitchClientError::new("Failed to write to output file."))?;
                }
            }
        }

        Ok(())
    }

    async fn download_video_part(
        &self,
        base_url: &String,
        video_part: &String,
    ) -> Result<(), TwitchClientError> {
        let req_url = base_url.to_owned() + &"/".to_string() + video_part;
        let res = self.client.get(req_url).send().await.map_err(|e| {
            TwitchClientError::new(&format!(
                "Failed to execute request for part {}, got error : {}",
                video_part, e
            ))
        })?;
        let res_bytes = res.bytes().await.map_err(|e| {
            TwitchClientError::new(&format!(
                "Failed to get response bytes for part {}, got error : {}",
                video_part, e
            ))
        })?;

        fs::write(Path::new(&self.temp_folder).join(video_part), res_bytes).map_err(|e| {
            TwitchClientError::new(&format!(
                "Failed to write response bytes for part {}, got error : {}",
                video_part, e
            ))
        })?;
        Ok(())
    }

    pub async fn get_vod_links(
        &self,
        download_options: &DownloadOptions,
    ) -> Result<HashMap<String, String>, TwitchClientError> {
        let video_id: String = download_options.get_video_id();
        let (access_token, signature) = self
            .get_access_token(video_id.clone())
            .await
            .map_err(|_| TwitchClientError::new("Unable to get Access Token"))?;
        let playlists = self
            .get_video_playlists(video_id, access_token, signature)
            .await
            .expect("Failed to get video playlists");

        if playlists[0].contains("vod_manifest_restricted") {
            panic!("Insufficient access to VOD, OAuth may be required.");
        }
        // Get Video Qualities
        let mut video_qualities: HashMap<String, String> = HashMap::<String, String>::new();
        for i in 0..playlists.len() {
            let playlist = playlists[i].to_string();
            if playlist.contains("#EXT-X-MEDIA") {
                let index_of_name = playlist.find("NAME=\"").unwrap() + 6;
                let lastpart = &playlist[index_of_name..];
                let quality_str = &lastpart[..lastpart.find("\"").unwrap()];
                if !video_qualities.contains_key(quality_str) {
                    video_qualities.insert(quality_str.to_string(), playlists[i + 2].to_string());
                }
            }
        }
        Ok(video_qualities)
    }
}
