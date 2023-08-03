use std::path::Path;

use anyhow::Result;

use vodfriends_rs::twitch::*;

#[tokio::main]
async fn main() -> Result<()> {
    let video_id = "1888413079";
    let mut twitch_client = VodFriendClient::new();
    twitch_client.set_temp_directory(Path::new("./temp/"));
    let download_options = DownloadOptions::new(video_id.to_string());
    let playlists = twitch_client.get_vod_links(&download_options).await?;
    twitch_client.progress_callback = Box::new(|download_progress| {
        println!(
            "Current Progress on {} is {}",
            video_id.to_string(),
            download_progress.get(&video_id.to_string()).unwrap_or(&0.0)
        );
    });
    twitch_client
        .download_vod(
            playlists.get("720p60").unwrap().to_string(),
            download_options,
        )
        .await?;

    // twitch_client
    //     .download_vod(
    //         playlists.get("720p60").unwrap().to_string(),
    //         download_options,
    //     )
    //     .await?;
    Ok(())
}
