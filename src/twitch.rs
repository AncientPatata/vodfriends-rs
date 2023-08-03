use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::*;

use chrono::{NaiveDateTime, Utc};
use serde_json::json;

const CLIENT_ID: &str = "kd1unb4b3q4t58fwlpcbzcbnm76a8fp";
const TWITCH_GQL_API: &str = "https://gql.twitch.tv/gql";

pub const GQL_GET_ACCESS_TOKEN_QUERY: &str = r#"query PlaybackAccessToken_Template(
    $login: String!,
    $isLive: Boolean!,
    $vodID: ID!,
    $isVod: Boolean!,
    $playerType: String!
  ) {
    streamPlaybackAccessToken(
      channelName: $login,
      params: {
        platform: "web",
        playerBackend: "mediaplayer",
        playerType: $playerType
      }
    ) @include(if: $isLive) {
      value
      signature
      __typename
    }
    videoPlaybackAccessToken(
      id: $vodID,
      params: {
        platform: "web",
        playerBackend: "mediaplayer",
        playerType: $playerType
      }
    ) @include(if: $isVod) {
      value
      signature
      __typename
    }
  }
  "#;

// TODO : Option to get a specific section (timestamp to timestamp of the stream ...etc )

type ProgressCallback = dyn Fn(&HashMap<String, f64>);

pub struct DownloadOptions {
    pub oauth_token: String,
    pub video_id: String,
    pub filename: String,
    pub filepath: String,
    pub quality: String,
}

impl DownloadOptions {
    pub fn new(video_id: String) -> DownloadOptions {
        DownloadOptions {
            oauth_token: "".to_string(),
            video_id: video_id.to_string(),
            quality: "720p60".to_string(),
            filename: video_id.to_string(),
            filepath: "./downloads/".to_string(),
        }
    }
    pub fn get_video_id(&self) -> String {
        self.video_id.to_string()
    }
    pub fn get_output_filename(&self) -> String {
        self.filename.to_string()
    }
    pub fn get_output_filepath(&self) -> String {
        self.filepath.to_string()
    }
}

pub struct VodFriendClient {
    pub client: reqwest::Client,
    temporary_folder: PathBuf,
    pub download_progress: HashMap<String, f64>,
    pub progress_callback: Box<ProgressCallback>,
}

impl VodFriendClient {
    pub fn new() -> VodFriendClient {
        VodFriendClient {
            client: reqwest::Client::new(),
            temporary_folder: PathBuf::new(),
            download_progress: HashMap::new(),
            progress_callback: Box::new(|_| {}),
        }
    }

    pub fn set_temp_directory(&mut self, temp_dir: &Path) {
        self.temporary_folder = PathBuf::from(temp_dir);
    }

    async fn get_access_token(&self, video_id: String) -> Result<(String, String)> {
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
        let json: serde_json::Value = response.json().await?;

        let access_token = json["data"]["videoPlaybackAccessToken"]["value"]
            .as_str()
            .ok_or(anyhow!(format!(
                "Failed to get access token for the video : {}",
                video_id
            )))?;

        let access_token_signature = json["data"]["videoPlaybackAccessToken"]["signature"]
            .as_str()
            .ok_or(anyhow!(format!(
                "Failed to get access token for the video : {}",
                video_id
            )))?;

        Ok((access_token.to_owned(), access_token_signature.to_owned()))
    }

    fn calculate_vod_age(video_chunks: &[&str]) -> Result<f64> {
        let id3_equiv_tdtg = video_chunks
            .iter()
            .find(|x| x.starts_with("#ID3-EQUIV-TDTG:"))
            .unwrap()
            .to_string();

        let timestamp = NaiveDateTime::parse_from_str(
            id3_equiv_tdtg
                .strip_prefix("#ID3-EQUIV-TDTG:")
                .ok_or(anyhow!(
                    "Couldn't strip prefix when trying to calculate VOD age",
                ))?
                .trim(),
            "%Y-%m-%dT%H:%M:%S",
        )
        .map_err(|e| {
            anyhow!(format!(
                "Couldn't get timestamp when trying to calculate VOD age, got ParseError: {}",
                e
            ))
            // TwitchClientError::new("Couldn't get timespamp when trying to calculate VOD age")
        })?;
        let current_time = Utc::now();
        let duration = current_time.signed_duration_since(timestamp.and_utc());
        let total_hours = duration.num_hours() as f64;
        Ok(total_hours)
    }

    pub async fn get_vod_links(
        &self,
        download_options: &DownloadOptions,
    ) -> Result<HashMap<String, String>> {
        let video_id: String = download_options.get_video_id();
        let (access_token, signature) = self
            .get_access_token(video_id.clone())
            .await
            .map_err(|e| anyhow!(format!("Unable to get Access Token, got error : {e}")))?;
        let playlists = self
            .get_video_playlists(video_id, access_token, signature)
            .await
            .map_err(|e| anyhow!(format!("Unable to get Access Token, got error : {e}")))?;

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

    pub async fn get_video_playlists(
        &self,
        video_id: String,
        access_token: String,
        signature: String,
    ) -> Result<Vec<String>> {
        let uri = format!(
            "http://usher.ttvnw.net/vod/{}?nauth={}&nauthsig={}&allow_source=true&player=twitchweb",
            video_id, access_token, signature
        );
        let playlist_str = self
            .client
            .get(uri)
            .header("Client-ID", CLIENT_ID)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let playlists: Vec<String> = playlist_str.split("\n").map(|x| x.to_string()).collect();
        Ok(playlists)
    }

    async fn download_video_part(&self, base_url: &String, video_part: &String) -> Result<()> {
        let req_url = base_url.to_owned() + &"/".to_string() + video_part;
        let res = self.client.get(req_url).send().await.map_err(|e| {
            anyhow!(format!(
                "Failed to execute request for part {}, got error : {}",
                video_part, e
            ))
        })?;
        let res_bytes = res.bytes().await.map_err(|e| {
            anyhow!(format!(
                "Failed to get response bytes for part {}, got error : {}",
                video_part, e
            ))
        })?;

        fs::write(self.temporary_folder.as_path().join(video_part), res_bytes).map_err(|e| {
            anyhow!(format!(
                "Failed to write response bytes for part {}, got error : {}",
                video_part, e
            ))
        })?;
        Ok(())
    }

    pub async fn download_vod(
        &mut self,
        playlist_url: String,
        download_options: DownloadOptions,
    ) -> Result<()> {
        let res = self
            .client
            .get(&playlist_url)
            .send()
            .await
            .map_err(|e| anyhow!(format!("Couldn't get video chunks : {}", e)))?
            .text()
            .await
            .map_err(|e| anyhow!(format!("Couldn't get video chunks : {}", e)))?;
        let video_chunks: Vec<&str> = res.split("\n").collect();
        let vod_age: f64 = Self::calculate_vod_age(&video_chunks)?;
        let unmute = vod_age < 24.0; // We check if the VOD is less than 24 hours old, if so, we try to get the unmuted version
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
                                + video_chunks[i][8..].trim_end_matches(",").parse::<f64>()?),
                        )
                    } else {
                        video_list.insert(
                            video_chunks[i + 2].to_string(),
                            video_chunks[i][8..].trim_end_matches(",").parse::<f64>()?,
                        );
                    }
                } else {
                    video_list.insert(
                        video_chunks[i + 1].to_string(),
                        video_chunks[i][8..].trim_end_matches(",").parse::<f64>()?,
                    );
                }
            }
        }
        //println!("Number of Video Parts : {}", video_list.len());
        fs::create_dir_all(self.temporary_folder.as_path()).map_err(|e| {
            anyhow!(format!(
                "Failed to create download directory, encountered error : {}",
                e
            ))
        })?;
        let total_video_parts = video_list.len();
        let mut curr_video_parts = 0;
        // Start downloading parts :
        for part in video_list {
            let (partname, _) = part;
            if unmute && partname.contains("-muted") {
                self.download_video_part(
                    &(playlist_url[0..playlist_url
                        .rfind("/")
                        .ok_or(anyhow!("Failed to download video part : {partname}"))?]
                        .to_string()),
                    &partname.replace("-muted", ""),
                )
                .await?;
            } else {
                self.download_video_part(
                    &(playlist_url[0..playlist_url
                        .rfind("/")
                        .ok_or(anyhow!("Failed to download video part : {partname}"))?]
                        .to_string()), // TODO : Add in Retry logic and give the user the option to skip on fail or store the parts temporarily so they can continue/resume later
                    &partname,
                )
                .await?;
            }
            curr_video_parts += 1;
            self.download_progress
                .entry(download_options.video_id.clone())
                .or_insert(curr_video_parts as f64 / total_video_parts as f64);
            (self.progress_callback)(&self.download_progress)
        }

        self.combine_video_parts(download_options).await?;

        Ok(())
    }

    async fn combine_video_parts(&self, download_options: DownloadOptions) -> Result<()> {
        let files = fs::read_dir(self.temporary_folder.as_path())
            .map_err(|e| anyhow!(format!("Failed to read directory, got error : {}", e)))?;
        let download_dir = Path::new(&download_options.filepath);
        fs::create_dir_all(download_dir).map_err(|e| {
            anyhow!(format!(
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
                anyhow!(format!(
                    "Failed to create output file, encountered error : {}",
                    e
                ))
            })?;

        for file in files {
            let file =
                file.map_err(|e| anyhow!(format!("Failed to read directory, got error : {e}")))?;
            let file_path = file.path();

            if file_path.is_file()
                && file_path.extension().is_some()
                && file_path.extension().unwrap() == "ts"
            {
                let mut input_file = fs::File::open(&file_path)
                    .map_err(|e| anyhow!(format!("Failed to open chunk file, got error : {e}")))?;

                // Read input file and write its contents to the output file
                let mut buffer = vec![0; 1024]; // Buffer size for reading
                loop {
                    let bytes_read = input_file.read(&mut buffer).map_err(|e| {
                        anyhow!(format!("Failed to read chunk file, got error : {e}"))
                    })?;
                    if bytes_read == 0 {
                        break;
                    }
                    output_file.write_all(&buffer[..bytes_read]).map_err(|e| {
                        anyhow!(format!("Failed to write to output file, got error: {e}"))
                    })?;
                }
            }
        }

        Ok(())
    }
}
