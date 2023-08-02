mod twitch;

use twitch::{
    download_options::DownloadOptions, requests::TwitchClient, requests::VideoInfo,
    utils::TwitchClientError,
};

#[tokio::main]
async fn main() -> Result<(), TwitchClientError> {
    let video_id = "SOME VIDEO ID";
    let mut twitch_client = TwitchClient::new();
    let download_options = DownloadOptions::new(video_id.to_string());
    let playlists = twitch_client.get_vod_links(&download_options).await?;
    twitch_client.set_progress_callback(|vid_info, prog| {
        println!(
            "Video with id {} is currently at {}%",
            vid_info.video_id, prog
        );
    });
    twitch_client
        .download_vod(
            playlists.get("720p60").unwrap().to_string(),
            download_options,
        )
        .await?;
    Ok(())
}
