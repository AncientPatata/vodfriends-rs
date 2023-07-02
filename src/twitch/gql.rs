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
