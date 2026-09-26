use super::*;
use std::{
    io::Cursor,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Barrier,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("siftly-copy-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }

    fn request(&self, sources: &[PathBuf], destination: &str) -> CopyRequest {
        CopyRequest {
            destination: self.0.join(destination).to_string_lossy().into_owned(),
            files: sources
                .iter()
                .map(|path| SelectedFile {
                    path: path.to_string_lossy().into_owned(),
                    size: path.metadata().map(|metadata| metadata.len()).unwrap_or(0),
                })
                .collect(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only this test's unique directory is owned here. Clear readonly on its
        // copied test files so Windows can remove the temporary fixture.
        fn clean(path: &Path) {
            for entry in fs::read_dir(path).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    clean(&path);
                } else {
                    #[cfg(windows)]
                    if let Ok(metadata) = path.metadata() {
                        let mut permissions = metadata.permissions();
                        // Windows-only test cleanup: clear FILE_ATTRIBUTE_READONLY.
                        #[allow(clippy::permissions_set_readonly_false)]
                        permissions.set_readonly(false);
                        let _ = fs::set_permissions(&path, permissions);
                    }
                }
            }
        }
        clean(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn collision_names_and_selected_order_preserve_every_original() {
    let fixture = Fixture::new();
    let one = fixture.file("one/clip.MP4", b"one");
    let two = fixture.file("two/clip.MP4", b"second");
    fixture.file("out/CLIP.mp4", b"existing");
    fs::create_dir(fixture.0.join("out/clip_1.MP4")).unwrap();
    let result = copy_files(
        fixture.request(&[one.clone(), two.clone()], "out"),
        || false,
        |_| {},
    )
    .unwrap();
    assert_eq!(
        (
            result.copied,
            result.failed,
            result.skipped,
            result.bytes_copied
        ),
        (2, 0, 0, 9)
    );
    assert_eq!(
        fs::read(fixture.0.join("out/CLIP.mp4")).unwrap(),
        b"existing"
    );
    assert_eq!(fs::read(fixture.0.join("out/clip_2.MP4")).unwrap(), b"one");
    assert_eq!(
        fs::read(fixture.0.join("out/clip_3.MP4")).unwrap(),
        b"second"
    );
    assert_eq!(fs::read(one).unwrap(), b"one");
    assert_eq!(fs::read(two).unwrap(), b"second");
}

#[test]
fn concurrent_reservations_cannot_overwrite_one_another() {
    let fixture = Fixture::new();
    let barrier = Arc::new(Barrier::new(4));
    let threads: Vec<_> = (0..4)
        .map(|index| {
            let directory = fixture.0.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut stale_names = HashSet::new();
                barrier.wait();
                let (path, mut file) =
                    reserve_destination(&directory, "clip.mp4", &mut stale_names, &|| false)
                        .unwrap()
                        .unwrap();
                file.write_all(&[index]).unwrap();
                (path, index)
            })
        })
        .collect();
    let results: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(
        results
            .iter()
            .map(|(path, _)| path)
            .collect::<HashSet<_>>()
            .len(),
        4
    );
    for (path, value) in results {
        assert_eq!(fs::read(path).unwrap(), vec![value]);
    }
}

#[test]
fn cancellation_mid_file_removes_partial_copy_and_rolls_back_bytes() {
    let fixture = Fixture::new();
    let source = fixture.file("source/large.mp4", &vec![7; CHUNK_SIZE * 3 + 5]);
    let next = fixture.file("source/next.mp4", b"next");
    let cancelled = AtomicBool::new(false);
    let mut events = Vec::new();
    let result = copy_files(
        fixture.request(&[source.clone(), next], "out"),
        || cancelled.load(Ordering::Relaxed),
        |progress| {
            if progress.bytes_copied > 0 {
                cancelled.store(true, Ordering::Relaxed);
            }
            events.push(progress);
        },
    )
    .unwrap();
    assert!(result.cancelled);
    assert_eq!(
        (
            result.copied,
            result.failed,
            result.skipped,
            result.bytes_copied
        ),
        (0, 0, 2, 0)
    );
    assert_eq!(fs::read_dir(fixture.0.join("out")).unwrap().count(), 0);
    assert_eq!(
        source.metadata().unwrap().len(),
        (CHUNK_SIZE * 3 + 5) as u64
    );
    assert!(events
        .iter()
        .any(|event| event.bytes_copied == CHUNK_SIZE as u64 && event.completed == 0));
    assert_eq!(events.last().unwrap().bytes_copied, 0);
}

#[test]
fn cancellation_between_files_retains_finished_copy() {
    let fixture = Fixture::new();
    let first = fixture.file("first.mp4", b"first");
    let second = fixture.file("second.mp4", b"second");
    let cancelled = AtomicBool::new(false);
    let result = copy_files(
        fixture.request(&[first, second], "out"),
        || cancelled.load(Ordering::Relaxed),
        |event| {
            if event.completed == 1 {
                cancelled.store(true, Ordering::Relaxed);
            }
        },
    )
    .unwrap();
    assert!(result.cancelled);
    assert_eq!(
        (result.copied, result.skipped, result.bytes_copied),
        (1, 1, 5)
    );
    assert_eq!(fs::read(fixture.0.join("out/first.mp4")).unwrap(), b"first");
    assert!(!fixture.0.join("out/second.mp4").exists());
}

#[test]
fn pre_cancel_and_empty_selection_do_not_create_a_destination() {
    let fixture = Fixture::new();
    let source = fixture.file("first", b"data");
    let result = copy_files(fixture.request(&[source], "out"), || true, |_| {}).unwrap();
    assert!(result.cancelled);
    assert_eq!(result.skipped, 1);
    copy_files(fixture.request(&[], "out"), || false, |_| {}).unwrap();
    assert!(!fixture.0.join("out").exists());
}

#[test]
fn missing_source_is_reported_and_remaining_files_are_copied() {
    let fixture = Fixture::new();
    let good = fixture.file("good.mp4", b"good");
    let result = copy_files(
        fixture.request(&[fixture.0.join("missing.mp4"), good], "out"),
        || false,
        |_| {},
    )
    .unwrap();
    assert_eq!((result.copied, result.failed, result.skipped), (1, 1, 0));
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].path.ends_with("missing.mp4"));
    assert_eq!(fs::read_dir(fixture.0.join("out")).unwrap().count(), 1);
}

#[test]
fn invalid_destination_is_fatal_and_source_is_intact() {
    let fixture = Fixture::new();
    let source = fixture.file("source", b"original");
    fixture.file("out", b"not a directory");
    let error = copy_files(
        fixture.request(std::slice::from_ref(&source), "out"),
        || false,
        |_| {},
    )
    .unwrap_err();
    assert!(error.contains("destination folder"));
    assert_eq!(fs::read(source).unwrap(), b"original");
}

#[test]
fn same_folder_and_zero_length_files_are_supported() {
    let fixture = Fixture::new();
    let source = fixture.file("empty", b"");
    let result = copy_files(fixture.request(&[source], ""), || false, |_| {}).unwrap();
    assert_eq!((result.copied, result.bytes_copied), (1, 0));
    assert!(fixture.0.join("empty_1").is_file());
}

#[test]
fn chunk_transfer_reports_io_errors_without_claiming_success() {
    struct DiskFull;
    impl Write for DiskFull {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("disk full"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = [0; 4];
    let mut input = Cursor::new(b"contents");
    assert!(transfer(&mut input, &mut DiskFull, 8, &mut buffer, &|| false, |_| {}).is_err());
    let mut short_input = Cursor::new(b"short");
    assert_eq!(
        transfer(
            &mut short_input,
            &mut Vec::new(),
            9,
            &mut buffer,
            &|| false,
            |_| {}
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::UnexpectedEof
    );
}

#[test]
fn progress_uses_actual_source_size_and_committed_total() {
    let fixture = Fixture::new();
    let source = fixture.file("large", &vec![4; CHUNK_SIZE + 9]);
    let mut request = fixture.request(&[source], "out");
    request.files[0].size = 1; // stale scan size
    let mut events = Vec::new();
    let result = copy_files(request, || false, |event| events.push(event)).unwrap();
    assert_eq!(result.bytes_copied, (CHUNK_SIZE + 9) as u64);
    let last = events.last().unwrap();
    assert_eq!(
        (last.bytes_copied, last.total_bytes, last.completed),
        (result.bytes_copied, result.bytes_copied, 1)
    );
}

#[cfg(windows)]
#[test]
fn windows_locked_source_fails_and_queue_continues() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let locked = fixture.file("locked", b"locked");
    let good = fixture.file("good", b"good");
    let _lock = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&locked)
        .unwrap();
    let result = copy_files(fixture.request(&[locked, good], "out"), || false, |_| {}).unwrap();
    assert_eq!((result.failed, result.copied), (1, 1));
    assert_eq!(result.errors.len(), 1);
    assert!(!fixture.0.join("out/locked").exists());
}

fn set_fixture_times(path: &Path) {
    let file = OpenOptions::new().write(true).open(path).unwrap();
    let times = FileTimes::new()
        .set_accessed(UNIX_EPOCH + Duration::from_secs(1_600_000_000))
        .set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_000));
    #[cfg(windows)]
    let times = {
        use std::os::windows::fs::FileTimesExt;
        times.set_created(UNIX_EPOCH + Duration::from_secs(1_500_000_000))
    };
    file.set_times(times).unwrap();
    #[cfg(windows)]
    set_attributes(&file, ATTRIBUTE_MASK).unwrap();
}

#[test]
fn native_command_scan_select_copy_flow() {
    let fixture = Fixture::new();
    fixture.file("source/selected.mp4", b"selected bytes");
    fixture.file("source/unselected.mp4", b"not selected");
    fixture.file("source/ignored.txt", b"ignored extension");
    let flag = AtomicBool::new(false);
    let mut scan_events = Vec::new();
    let scan = crate::execute_operation(
        "scan",
        serde_json::json!({
            "source": fixture.0.join("source"), "extensions": ["mp4"],
            "date_from": null, "date_to": null,
        }),
        &flag,
        |event| scan_events.push(event),
    )
    .unwrap();
    assert_eq!(scan["files"].as_array().unwrap().len(), 2);
    assert_eq!(scan_events.last().unwrap()["type"], "scan_progress");
    let selected = scan["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["name"] == "selected.mp4")
        .unwrap();
    let mut copy_events = Vec::new();
    let result = crate::execute_operation(
        "copy",
        serde_json::json!({
            "destination": fixture.0.join("out"), "files": [selected],
        }),
        &flag,
        |event| copy_events.push(event),
    )
    .unwrap();
    assert_eq!(result["copied"], 1);
    assert_eq!(result["failed"], 0);
    assert_eq!(copy_events.last().unwrap()["type"], "copy_progress");
    assert_eq!(fs::read_dir(fixture.0.join("out")).unwrap().count(), 1);
    assert_eq!(
        fs::read(fixture.0.join("out/selected.mp4")).unwrap(),
        b"selected bytes"
    );
}

fn assert_metadata_equal(left: &Metadata, right: &Metadata) {
    assert_eq!(left.len(), right.len());
    assert_eq!(left.modified().unwrap(), right.modified().unwrap());
    assert_eq!(left.accessed().unwrap(), right.accessed().unwrap());
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        assert_eq!(left.created().unwrap(), right.created().unwrap());
        assert_eq!(
            left.file_attributes() & ATTRIBUTE_MASK,
            right.file_attributes() & ATTRIBUTE_MASK
        );
    }
}

#[test]
fn preserves_dates_and_supported_attributes() {
    let fixture = Fixture::new();
    let source = fixture.file("source/метаданные.mp4", b"metadata");
    set_fixture_times(&source);
    let before = source.metadata().unwrap();
    let result = copy_files(
        fixture.request(std::slice::from_ref(&source), "out"),
        || false,
        |_| {},
    )
    .unwrap();
    assert_eq!(result.copied, 1, "{:?}", result.errors);
    let copied = fixture.0.join("out/метаданные.mp4");
    assert_metadata_equal(&before, &copied.metadata().unwrap());
    assert_eq!(fs::read(copied).unwrap(), fs::read(source).unwrap());
}

#[test]
fn copy_result_contract_matches_fixed_fixture() {
    let fixture = Fixture::new();
    let first = fixture.file("one/clip.mp4", b"first");
    let second = fixture.file("two/clip.mp4", b"second");
    let empty = fixture.file("one/empty", b"");
    let sources = [first, second, empty];
    for source in &sources {
        set_fixture_times(source);
    }
    let expected_metadata: Vec<_> = sources
        .iter()
        .map(|source| source.metadata().unwrap())
        .collect();
    fixture.file("out/clip.mp4", b"existing");
    fixture.file("out/clip_1.mp4", b"also existing");

    let result = copy_files(fixture.request(&sources, "out"), || false, |_| {}).unwrap();
    let mut actual = serde_json::to_value(&result).unwrap();
    assert!(actual["duration"].as_f64().unwrap() >= 0.0);
    actual.as_object_mut().unwrap().remove("duration");
    assert_eq!(
        actual,
        serde_json::json!({
            "type": "copy_result", "total": 3, "copied": 3, "failed": 0,
            "skipped": 0, "bytes_copied": 11, "cancelled": false, "errors": []
        })
    );
    for (index, (name, bytes)) in [
        ("clip_2.mp4", b"first".as_slice()),
        ("clip_3.mp4", b"second"),
        ("empty", b""),
    ]
    .iter()
    .enumerate()
    {
        let copied = fixture.0.join("out").join(name);
        assert_metadata_equal(&expected_metadata[index], &copied.metadata().unwrap());
        assert_eq!(fs::read(copied).unwrap(), *bytes);
    }
    assert_eq!(
        fs::read(fixture.0.join("out/clip.mp4")).unwrap(),
        b"existing"
    );
    assert_eq!(
        fs::read(fixture.0.join("out/clip_1.mp4")).unwrap(),
        b"also existing"
    );
}
