# Setup's tests

- `tests.rs`: the files in `tests/` that hold the specs of `setup.rs`, and what they share: the downloads' hosts faked, an install's events heard, and the speech models as an install leaves them.
- `tests/first_run.rs`: what setup says the computer has, setup's run and its cancel, and yt-dlp's update.
- `tests/gpu_add_on.rs`: the GPU add-on: what it says, its install, its cancel and its removal.
- `tests/settings.rs`: the settings commands: reading, saving and resetting them, a file that can't be read, and the formats the ffmpeg in use can't make.
- `tests/storage.rs`: the downloads' size and Repair, the kept videos' size, the data folder, and what this computer plays.
