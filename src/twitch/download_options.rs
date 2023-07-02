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
