# CMS Video Recorder

A simple video recorder designed for official MapleStory clients, specifically ChinaMS clients.

*This project is not officially supported by SQ Games yet*

## Why it's a thing?

I want the procedure of video recording for reporting players who're violating ToS easier.

The report website only allows uploading videos less than 5MB, and most of players probably don't know how to compress video with FFmpeg.

## Building
1. Download and install Visual Studio Build Tools and Git.
- https://aka.ms/vs/stable/vs_BuildTools.exe
- https://git-scm.com/install/windows

2. Install Rust by downloading rustup-init.exe from Rust website.
- https://rust-lang.org/learn/get-started/

3. Clone this repository.
```pwsh
git clone https://github.com/HikariCalyx/cms_video_recorder
cd cms_video_recorder
```

4. Build it.
```pwsh
cargo build --release
```

5. Place ffmpeg.exe binary along with it. This project uses the prebuilt FFmpeg binary provided for Trill 5 project:

https://github.com/HCTOrganization/ffmpeg-build/

## Licenses
- CMS Video Recorder: MIT
- stripped down FFmpeg build used for this project: GPLv3