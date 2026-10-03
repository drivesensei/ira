//! D-003 intentional safety exception: frozen oracle remains unchanged.
use super::*;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "ira-overwrite-safety-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join("dst")).unwrap();
        Self(root)
    }
    fn path(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn transfer(
    f: &Fixture,
    kind: JobKind,
    name: &str,
    policy: OverwritePolicy,
) -> Result<(), JobError> {
    let (tx, _rx) = mpsc::channel();
    run_batch(
        1,
        kind,
        &[f.path(&format!("src/{name}"))
            .to_string_lossy()
            .into_owned()],
        &f.path("dst"),
        policy,
        &JobControl::new(),
        &tx,
    )
}
#[test]
fn overwrite_directory_failure_preserves_existing_tree_and_source() {
    for kind in [JobKind::Copy, JobKind::Move] {
        let f = Fixture::new();
        fs::create_dir(f.path("src/tree")).unwrap();
        fs::write(f.path("src/tree/source.txt"), b"SOURCE").unwrap();
        fs::create_dir(f.path("dst/tree")).unwrap();
        fs::write(f.path("dst/tree/precious.txt"), b"PRECIOUS").unwrap();
        assert!(transfer(&f, kind, "tree", OverwritePolicy::Overwrite).is_err());
        assert_eq!(
            fs::read(f.path("dst/tree/precious.txt")).unwrap(),
            b"PRECIOUS"
        );
        assert_eq!(fs::read(f.path("src/tree/source.txt")).unwrap(), b"SOURCE");
    }
}
#[test]
fn missing_source_overwrite_failure_preserves_existing_file() {
    for kind in [JobKind::Copy, JobKind::Move] {
        let f = Fixture::new();
        fs::write(f.path("dst/absent.txt"), b"PRECIOUS").unwrap();
        assert!(transfer(&f, kind, "absent.txt", OverwritePolicy::Overwrite).is_err());
        assert_eq!(fs::read(f.path("dst/absent.txt")).unwrap(), b"PRECIOUS");
    }
}
#[test]
fn successful_file_overwrite_preserves_copy_and_move_semantics() {
    for kind in [JobKind::Copy, JobKind::Move] {
        let f = Fixture::new();
        fs::write(f.path("src/file.txt"), b"NEW").unwrap();
        fs::write(f.path("dst/file.txt"), b"OLD").unwrap();
        transfer(&f, kind, "file.txt", OverwritePolicy::Overwrite).unwrap();
        assert_eq!(fs::read(f.path("dst/file.txt")).unwrap(), b"NEW");
        assert_eq!(f.path("src/file.txt").exists(), kind == JobKind::Copy);
    }
}
#[cfg(unix)]
#[test]
fn failed_overwrite_preserves_destination_symlink_and_its_target() {
    let f = Fixture::new();
    fs::write(f.path("precious.txt"), b"PRECIOUS").unwrap();
    create_symlink("../precious.txt", f.path("dst/absent.txt")).unwrap();
    assert!(transfer(&f, JobKind::Copy, "absent.txt", OverwritePolicy::Overwrite).is_err());
    assert_eq!(
        fs::read_link(f.path("dst/absent.txt")).unwrap(),
        Path::new("../precious.txt")
    );
    assert_eq!(fs::read(f.path("precious.txt")).unwrap(), b"PRECIOUS");
}

fn assert_no_stages(f: &Fixture) {
    assert!(fs::read_dir(f.path("dst")).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".ira-transfer-")));
}
#[test]
fn staged_publication_race_preserves_intervening_file_and_directory() {
    for directory in [false, true] {
        let f = Fixture::new();
        let src = f.path("src/tree");
        if directory {
            fs::create_dir(&src).unwrap();
            fs::write(src.join("source.txt"), b"SOURCE").unwrap();
        } else {
            fs::write(&src, b"SOURCE").unwrap();
        }
        let dst = resolve_destination(&src, &f.path("dst"), OverwritePolicy::AutoRename).unwrap();
        let control = JobControl::new();
        let (tx, _rx) = mpsc::channel();
        let mut bytes = 0;
        let mut context = TransferContext {
            control: &control,
            id: 1,
            tx: &tx,
            bytes: &mut bytes,
            provider: &rename_no_replace,
        };
        let result = copy_staged(&src, &dst, false, &mut context, |_| {
            // Replace an intervening destination again before publication.
            if directory {
                fs::create_dir(&dst).unwrap();
                fs::write(dst.join("actor.txt"), b"FIRST").unwrap();
            } else {
                fs::write(&dst, b"FIRST").unwrap();
            }
            fs::rename(&dst, f.path("actor-backup")).unwrap();
            if directory {
                fs::create_dir(&dst).unwrap();
                fs::write(dst.join("actor.txt"), b"SECOND").unwrap();
            } else {
                fs::write(&dst, b"SECOND").unwrap();
            }
        });
        assert!(result.is_err());
        let current = if directory {
            dst.join("actor.txt")
        } else {
            dst.clone()
        };
        assert_eq!(fs::read(current).unwrap(), b"SECOND");
        assert!(src.exists());
        assert_no_stages(&f);
    }
}
#[test]
fn cancellation_before_publication_preserves_source_existing_target_and_cleans_stage() {
    let f = Fixture::new();
    let src = f.path("src/file.txt");
    let dst = f.path("dst/file.txt");
    fs::write(&src, b"SOURCE").unwrap();
    fs::write(&dst, b"PRECIOUS").unwrap();
    let control = JobControl::new();
    let (tx, _rx) = mpsc::channel();
    let mut bytes = 0;
    let mut context = TransferContext {
        control: &control,
        id: 1,
        tx: &tx,
        bytes: &mut bytes,
        provider: &rename_no_replace,
    };
    let result = copy_staged(&src, &dst, true, &mut context, |payload| {
        assert_eq!(fs::read(payload).unwrap(), b"SOURCE");
        assert_eq!(fs::read(&dst).unwrap(), b"PRECIOUS");
        control.request_cancel();
    });
    assert!(matches!(result, Err(JobError::Cancelled)));
    assert_eq!(fs::read(src).unwrap(), b"SOURCE");
    assert_eq!(fs::read(dst).unwrap(), b"PRECIOUS");
    assert_no_stages(&f);
}
#[cfg(unix)]
#[test]
fn directory_partial_copy_failure_cleans_only_private_stage() {
    let f = Fixture::new();
    fs::create_dir(f.path("src/tree")).unwrap();
    fs::write(f.path("src/tree/good.txt"), b"SOURCE").unwrap();
    let _listener = std::os::unix::net::UnixListener::bind(f.path("src/tree/socket")).unwrap();
    assert!(transfer(&f, JobKind::Copy, "tree", OverwritePolicy::AutoRename).is_err());
    assert_eq!(fs::read(f.path("src/tree/good.txt")).unwrap(), b"SOURCE");
    assert!(!f.path("dst/tree").exists());
    assert_no_stages(&f);
}

#[test]
fn directory_over_file_preserves_legacy_success_with_recoverable_backup() {
    for kind in [JobKind::Copy, JobKind::Move] {
        let f = Fixture::new();
        fs::create_dir(f.path("src/tree")).unwrap();
        fs::write(f.path("src/tree/source.txt"), b"SOURCE").unwrap();
        fs::write(f.path("dst/tree"), b"PRECIOUS").unwrap();
        transfer(&f, kind, "tree", OverwritePolicy::Overwrite).unwrap();
        assert_eq!(fs::read(f.path("dst/tree/source.txt")).unwrap(), b"SOURCE");
        assert_eq!(f.path("src/tree").exists(), kind == JobKind::Copy);
        assert_no_stages(&f);
    }
}

fn recovery_transfer(
    f: &Fixture,
    kind: JobKind,
    force_copy_move: bool,
    hook: impl FnMut(StagePoint, &Path) -> Result<(), JobError>,
) -> Result<(), JobError> {
    let control = JobControl::new();
    let (tx, _rx) = mpsc::channel();
    let mut bytes = 0;
    let mut context = TransferContext {
        control: &control,
        id: 1,
        tx: &tx,
        bytes: &mut bytes,
        provider: &rename_no_replace,
    };
    transfer_staged(
        &f.path("src/file"),
        &f.path("dst/file"),
        kind,
        true,
        &mut context,
        force_copy_move,
        hook,
    )
}
fn stages(f: &Fixture) -> Vec<PathBuf> {
    fs::read_dir(f.path("dst"))
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".ira-transfer-")
                .then(|| entry.path())
        })
        .collect()
}
#[test]
fn publication_failure_restores_backup_and_fast_move_source() {
    for kind in [JobKind::Copy, JobKind::Move] {
        let f = Fixture::new();
        fs::write(f.path("src/file"), b"SOURCE").unwrap();
        fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
        let result = recovery_transfer(&f, kind, false, |point, _| {
            if point == StagePoint::BeforePublication {
                return Err(JobError::Io("injected publication failure".into()));
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(fs::read(f.path("src/file")).unwrap(), b"SOURCE");
        assert_eq!(fs::read(f.path("dst/file")).unwrap(), b"PRECIOUS");
        assert_no_stages(&f);
    }
}
#[test]
fn restoration_collision_preserves_actor_and_retains_recoverable_backup() {
    for kind in [JobKind::Copy, JobKind::Move] {
        let f = Fixture::new();
        fs::write(f.path("src/file"), b"SOURCE").unwrap();
        fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
        let result = recovery_transfer(&f, kind, false, |point, _| {
            if point == StagePoint::BackupCaptured {
                fs::write(f.path("dst/file"), b"ACTOR").unwrap();
            }
            Ok(())
        });
        let error = result.unwrap_err();
        assert_eq!(fs::read(f.path("src/file")).unwrap(), b"SOURCE");
        assert_eq!(fs::read(f.path("dst/file")).unwrap(), b"ACTOR");
        let retained = stages(&f);
        assert_eq!(retained.len(), 1);
        assert_eq!(fs::read(retained[0].join("backup")).unwrap(), b"PRECIOUS");
        assert!(format!("{error:?}").contains(&retained[0].display().to_string()));
    }
}
#[test]
fn unexpected_captured_directory_is_restored_without_deleting_contents() {
    for kind in [JobKind::Copy, JobKind::Move] {
        let f = Fixture::new();
        fs::write(f.path("src/file"), b"SOURCE").unwrap();
        fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
        let result = recovery_transfer(&f, kind, false, |point, _| {
            if point == StagePoint::PayloadReady {
                fs::rename(f.path("dst/file"), f.path("actor-saved-file")).unwrap();
                fs::create_dir(f.path("dst/file")).unwrap();
                fs::write(f.path("dst/file/actor"), b"ACTOR").unwrap();
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(fs::read(f.path("src/file")).unwrap(), b"SOURCE");
        assert_eq!(fs::read(f.path("dst/file/actor")).unwrap(), b"ACTOR");
        assert_eq!(fs::read(f.path("actor-saved-file")).unwrap(), b"PRECIOUS");
        assert_no_stages(&f);
    }
}
#[test]
fn fallback_move_postpublication_failure_preserves_commit_and_both_originals() {
    let f = Fixture::new();
    fs::write(f.path("src/file"), b"SOURCE").unwrap();
    fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
    let error = recovery_transfer(&f, JobKind::Move, true, |point, _| {
        if point == StagePoint::Published {
            return Err(JobError::Io("injected postcommit failure".into()));
        }
        Ok(())
    })
    .unwrap_err();
    assert!(!f.path("src/file").exists());
    assert_eq!(fs::read(f.path("dst/file")).unwrap(), b"SOURCE");
    let retained = stages(&f);
    assert_eq!(retained.len(), 1);
    assert_eq!(fs::read(retained[0].join("backup")).unwrap(), b"PRECIOUS");
    let source = source_stages(&f);
    assert_eq!(source.len(), 1);
    assert_eq!(fs::read(source[0].join("captured")).unwrap(), b"SOURCE");
    assert!(matches!(error, JobError::CommittedWithRecovery(_)));
    for path in [f.path("dst/file"), retained[0].clone(), source[0].clone()] {
        assert!(format!("{error:?}").contains(&path.display().to_string()));
    }
}

#[test]
fn published_replacement_remains_public_and_old_backup_is_retained() {
    let f = Fixture::new();
    fs::write(f.path("src/file"), b"SOURCE").unwrap();
    fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
    let result = recovery_transfer(&f, JobKind::Copy, false, |point, _| {
        if point == StagePoint::Published {
            fs::rename(f.path("dst/file"), f.path("actor-saved-payload")).unwrap();
            fs::write(f.path("dst/file"), b"ACTOR").unwrap();
            return Err(JobError::Io("injected commit failure".into()));
        }
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(fs::read(f.path("dst/file")).unwrap(), b"ACTOR");
    assert_eq!(fs::read(f.path("actor-saved-payload")).unwrap(), b"SOURCE");
    let retained = stages(&f);
    assert_eq!(retained.len(), 1);
    assert_eq!(fs::read(retained[0].join("backup")).unwrap(), b"PRECIOUS");
}

#[cfg(unix)]
#[test]
fn move_overwrite_empty_directory_preserves_legacy_success() {
    let f = Fixture::new();
    fs::create_dir(f.path("src/tree")).unwrap();
    fs::write(f.path("src/tree/source.txt"), b"SOURCE").unwrap();
    fs::create_dir(f.path("dst/tree")).unwrap();
    transfer(&f, JobKind::Move, "tree", OverwritePolicy::Overwrite).unwrap();
    assert_eq!(fs::read(f.path("dst/tree/source.txt")).unwrap(), b"SOURCE");
    assert!(!f.path("src/tree").exists());
    assert_no_stages(&f);
}

#[cfg(unix)]
#[test]
fn directory_move_atomic_rename_refuses_concurrent_destination_fill() {
    let f = Fixture::new();
    let src = f.path("src/tree");
    let dst = f.path("dst/tree");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("source"), b"SOURCE").unwrap();
    fs::create_dir(&dst).unwrap();
    let control = JobControl::new();
    let (tx, _rx) = mpsc::channel();
    let mut bytes = 0;
    let mut context = TransferContext {
        control: &control,
        id: 1,
        tx: &tx,
        bytes: &mut bytes,
        provider: &rename_no_replace,
    };
    let result = move_directory_over_directory(&src, &dst, &mut context, || {
        fs::write(dst.join("actor"), b"PRECIOUS").unwrap();
    });
    assert!(result.is_err());
    assert_eq!(fs::read(dst.join("actor")).unwrap(), b"PRECIOUS");
    assert_eq!(fs::read(src.join("source")).unwrap(), b"SOURCE");
    assert_no_stages(&f);
}

#[test]
fn fallback_move_preserves_actor_replacement_source_entries() {
    for directory in [false, true] {
        let f = Fixture::new();
        let src = f.path("src/file");
        if directory {
            fs::create_dir(&src).unwrap();
            fs::write(src.join("original"), b"SOURCE").unwrap();
        } else {
            fs::write(&src, b"SOURCE").unwrap();
        }
        fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
        let _result = recovery_transfer(&f, JobKind::Move, true, |point, _| {
            if point == StagePoint::Published {
                // Before the fix the original is still public. A source-capture
                // implementation can already have moved it to private recovery.
                if src.exists() {
                    fs::rename(&src, f.path("original-source")).unwrap();
                }
                if directory {
                    fs::create_dir(&src).unwrap();
                    fs::write(src.join("actor"), b"ACTOR").unwrap();
                } else {
                    fs::write(&src, b"ACTOR").unwrap();
                }
            }
            Ok(())
        });
        let actor = if directory {
            src.join("actor")
        } else {
            src.clone()
        };
        assert_eq!(fs::read(actor).unwrap(), b"ACTOR");
    }
}
#[cfg(unix)]
#[test]
fn directory_move_dual_type_replacement_preserves_actor_target() {
    let f = Fixture::new();
    let src = f.path("src/tree");
    let dst = f.path("dst/tree");
    fs::create_dir(&src).unwrap();
    fs::create_dir(&dst).unwrap();
    let control = JobControl::new();
    let (tx, _rx) = mpsc::channel();
    let mut bytes = 0;
    let mut context = TransferContext {
        control: &control,
        id: 1,
        tx: &tx,
        bytes: &mut bytes,
        provider: &rename_no_replace,
    };
    let result = move_directory_over_directory(&src, &dst, &mut context, || {
        fs::rename(&src, f.path("saved-source-dir")).unwrap();
        fs::rename(&dst, f.path("saved-target-dir")).unwrap();
        fs::write(&src, b"ACTOR SOURCE").unwrap();
        fs::write(&dst, b"PRECIOUS ACTOR TARGET").unwrap();
    });
    assert!(
        result.is_err(),
        "type replacement must refuse the operation"
    );
    assert_eq!(fs::read(&dst).unwrap(), b"PRECIOUS ACTOR TARGET");
    assert_eq!(fs::read(&src).unwrap(), b"ACTOR SOURCE");
}

fn source_stages(f: &Fixture) -> Vec<PathBuf> {
    fs::read_dir(f.path("src"))
        .unwrap()
        .filter_map(|e| {
            let e = e.unwrap();
            e.file_name()
                .to_string_lossy()
                .starts_with(".ira-transfer-")
                .then(|| e.path())
        })
        .collect()
}

#[test]
fn cancellation_phases_restore_source_and_target_before_commit() {
    for phase in [
        StagePoint::BeforeSourceCapture,
        StagePoint::SourceCaptured,
        StagePoint::PayloadReady,
        StagePoint::BackupCaptured,
        StagePoint::BeforePublication,
    ] {
        let f = Fixture::new();
        fs::write(f.path("src/file"), b"SOURCE").unwrap();
        fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
        let result = recovery_transfer(&f, JobKind::Move, false, |point, _| {
            if point == phase {
                Err(JobError::Cancelled)
            } else {
                Ok(())
            }
        });
        assert!(
            matches!(result, Err(JobError::Cancelled)),
            "phase={phase:?},result={result:?}"
        );
        assert_eq!(fs::read(f.path("src/file")).unwrap(), b"SOURCE");
        assert_eq!(fs::read(f.path("dst/file")).unwrap(), b"PRECIOUS");
        assert_no_stages(&f);
        assert!(source_stages(&f).is_empty());
    }
}
#[test]
fn simultaneous_source_and_destination_restore_collisions_retain_both_originals() {
    let f = Fixture::new();
    fs::write(f.path("src/file"), b"SOURCE").unwrap();
    fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
    let error = recovery_transfer(&f, JobKind::Move, false, |point, _| {
        if point == StagePoint::BackupCaptured {
            fs::write(f.path("src/file"), b"ACTOR SOURCE").unwrap();
            fs::write(f.path("dst/file"), b"ACTOR TARGET").unwrap();
            return Err(JobError::Cancelled);
        }
        Ok(())
    })
    .unwrap_err();
    assert_eq!(fs::read(f.path("src/file")).unwrap(), b"ACTOR SOURCE");
    assert_eq!(fs::read(f.path("dst/file")).unwrap(), b"ACTOR TARGET");
    let retained = stages(&f);
    assert_eq!(retained.len(), 1);
    assert_eq!(fs::read(retained[0].join("payload")).unwrap(), b"SOURCE");
    assert_eq!(fs::read(retained[0].join("backup")).unwrap(), b"PRECIOUS");
    assert!(format!("{error:?}").contains(&retained[0].display().to_string()));
    assert!(fs::read_to_string(retained[0].join("recovery.txt"))
        .unwrap()
        .contains("restore incomplete"));
}
#[test]
fn copied_move_retains_held_handle_same_length_same_mtime_write() {
    use std::io::{Seek, SeekFrom, Write};
    let f = Fixture::new();
    let src = f.path("src/file");
    fs::write(&src, b"SOURCE").unwrap();
    fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
    let original_mtime = fs::metadata(&src).unwrap().modified().unwrap();
    let mut handle = OpenOptions::new().write(true).open(&src).unwrap();
    let error = recovery_transfer(&f, JobKind::Move, true, |point, _| {
        if point == StagePoint::PayloadReady {
            handle.seek(SeekFrom::Start(0)).unwrap();
            handle.write_all(b"LATEST").unwrap();
            handle
                .set_times(fs::FileTimes::new().set_modified(original_mtime))
                .unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(error, JobError::CommittedWithRecovery(_)));
    assert_eq!(fs::read(f.path("dst/file")).unwrap(), b"SOURCE");
    assert!(!src.exists());
    let retained = source_stages(&f);
    assert_eq!(retained.len(), 1);
    assert_eq!(fs::read(retained[0].join("captured")).unwrap(), b"LATEST");
    assert!(format!("{error:?}").contains("destination WAS published"));
    assert!(format!("{error:?}").contains(&retained[0].display().to_string()));
}
#[test]
fn copied_move_retains_source_directory_child_created_after_enumeration() {
    let f = Fixture::new();
    fs::create_dir(f.path("src/file")).unwrap();
    fs::write(f.path("src/file/original"), b"SOURCE").unwrap();
    fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
    let mut captured = None;
    let error = recovery_transfer(&f, JobKind::Move, true, |point, path| {
        if point == StagePoint::SourceCaptured {
            captured = Some(path.to_path_buf());
        }
        if point == StagePoint::PayloadReady {
            fs::write(captured.as_ref().unwrap().join("late"), b"LATE CHILD").unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(error, JobError::CommittedWithRecovery(_)));
    assert_eq!(fs::read(f.path("dst/file/original")).unwrap(), b"SOURCE");
    assert!(!f.path("dst/file/late").exists());
    let retained = source_stages(&f);
    assert_eq!(retained.len(), 1);
    assert_eq!(
        fs::read(retained[0].join("captured/late")).unwrap(),
        b"LATE CHILD"
    );
}
#[cfg(unix)]
#[test]
fn captured_empty_destination_filled_after_validation_is_never_recursively_deleted() {
    let f = Fixture::new();
    fs::create_dir(f.path("src/file")).unwrap();
    fs::write(f.path("src/file/original"), b"SOURCE").unwrap();
    fs::create_dir(f.path("dst/file")).unwrap();
    let error = recovery_transfer(&f, JobKind::Move, false, |point, path| {
        if point == StagePoint::Published {
            fs::write(
                path.parent().unwrap().join("backup/late"),
                b"PRECIOUS CHILD",
            )
            .unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(error, JobError::CommittedWithRecovery(_)));
    assert_eq!(fs::read(f.path("dst/file/original")).unwrap(), b"SOURCE");
    let retained = stages(&f);
    assert_eq!(retained.len(), 1);
    assert_eq!(
        fs::read(retained[0].join("backup/late")).unwrap(),
        b"PRECIOUS CHILD"
    );
}
#[test]
fn late_cancellation_keeps_commit_and_stops_remaining_batch() {
    let f = Fixture::new();
    fs::write(f.path("src/first"), b"FIRST").unwrap();
    fs::write(f.path("src/second"), b"SECOND").unwrap();
    let control = JobControl::new();
    let (tx, _rx) = mpsc::channel();
    let dst = f.path("dst/first");
    let captured_control = control.clone();
    let captured_dst = dst.clone();
    let provider = move |src: &Path, target: &Path| {
        rename_no_replace(src, target)?;
        if target == captured_dst {
            captured_control.request_cancel();
        }
        Ok(())
    };
    let result = run_batch_with_provider(
        1,
        JobKind::Move,
        &[
            f.path("src/first").to_string_lossy().into_owned(),
            f.path("src/second").to_string_lossy().into_owned(),
        ],
        &f.path("dst"),
        OverwritePolicy::AutoRename,
        &control,
        &tx,
        &provider,
    );
    assert!(
        matches!(result, Err(JobError::CommittedWithRecovery(_))),
        "{result:?}"
    );
    assert_eq!(fs::read(&dst).unwrap(), b"FIRST");
    assert!(!f.path("src/first").exists());
    assert_eq!(fs::read(f.path("src/second")).unwrap(), b"SECOND");
    assert!(!f.path("dst/second").exists());
}
#[cfg(unix)]
#[test]
fn captured_move_relative_symlinks_keep_original_text_inside_and_outside_tree() {
    let f = Fixture::new();
    fs::write(f.path("outside"), b"OUTSIDE").unwrap();
    fs::create_dir(f.path("src/file")).unwrap();
    fs::write(f.path("src/file/inside"), b"INSIDE").unwrap();
    create_symlink("inside", f.path("src/file/internal")).unwrap();
    create_symlink("../../outside", f.path("src/file/external")).unwrap();
    fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
    let error = recovery_transfer(&f, JobKind::Move, true, |_, _| Ok(())).unwrap_err();
    assert!(matches!(error, JobError::CommittedWithRecovery(_)));
    assert_eq!(
        fs::read_link(f.path("dst/file/internal")).unwrap(),
        Path::new("inside")
    );
    assert_eq!(
        fs::read_link(f.path("dst/file/external")).unwrap(),
        Path::new("../../outside")
    );
    assert_eq!(fs::read(f.path("dst/file/internal")).unwrap(), b"INSIDE");
    assert_eq!(fs::read(f.path("dst/file/external")).unwrap(), b"OUTSIDE");
    let retained = source_stages(&f);
    assert_eq!(
        fs::read_link(retained[0].join("captured/external")).unwrap(),
        Path::new("../../outside")
    );
}
#[test]
fn missing_provider_fails_without_namespace_mutation() {
    let f = Fixture::new();
    fs::write(f.path("src/file"), b"SOURCE").unwrap();
    fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
    let control = JobControl::new();
    let (tx, _rx) = mpsc::channel();
    let mut bytes = 0;
    let provider = |_: &Path, _: &Path| {
        Err(std::io::Error::new(
            ErrorKind::Unsupported,
            "missing backend",
        ))
    };
    let mut context = TransferContext {
        control: &control,
        id: 1,
        tx: &tx,
        bytes: &mut bytes,
        provider: &provider,
    };
    assert!(transfer_staged(
        &f.path("src/file"),
        &f.path("dst/file"),
        JobKind::Move,
        true,
        &mut context,
        false,
        |_, _| Ok(())
    )
    .is_err());
    assert_eq!(fs::read(f.path("src/file")).unwrap(), b"SOURCE");
    assert_eq!(fs::read(f.path("dst/file")).unwrap(), b"PRECIOUS");
    assert_no_stages(&f);
    assert!(source_stages(&f).is_empty());
}

#[cfg(unix)]
#[test]
fn copied_move_retains_child_written_through_captured_directory_descriptor() {
    use std::io::Write;
    let f = Fixture::new();
    fs::create_dir(f.path("src/file")).unwrap();
    fs::write(f.path("src/file/original"), b"SOURCE").unwrap();
    fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
    let directory_handle = File::open(f.path("src/file")).unwrap();
    let error = recovery_transfer(&f, JobKind::Move, true, |point, _| {
        if point == StagePoint::PayloadReady {
            let fd = rustix::fs::openat(
                &directory_handle,
                "late-dirfd",
                rustix::fs::OFlags::CREATE | rustix::fs::OFlags::EXCL | rustix::fs::OFlags::WRONLY,
                rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            )
            .unwrap();
            File::from(fd).write_all(b"DIRFD CHILD").unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(error, JobError::CommittedWithRecovery(_)));
    let retained = source_stages(&f);
    assert_eq!(retained.len(), 1);
    assert_eq!(
        fs::read(retained[0].join("captured/late-dirfd")).unwrap(),
        b"DIRFD CHILD"
    );
    assert!(!f.path("dst/file/late-dirfd").exists());
}

#[test]
fn simulated_exdev_copies_from_source_volume_capture_and_reports_recovery() {
    let f = Fixture::new();
    fs::write(f.path("src/file"), b"SOURCE").unwrap();
    fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
    let control = JobControl::new();
    let (tx, _rx) = mpsc::channel();
    let mut bytes = 0;
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed_provider = observed.clone();
    let provider = move |src: &Path, dst: &Path| {
        if src.file_name().is_some_and(|s| s == "captured")
            && dst.file_name().is_some_and(|s| s == "payload")
        {
            observed_provider.store(true, Ordering::Relaxed);
            return Err(std::io::Error::from(ErrorKind::CrossesDevices));
        }
        rename_no_replace(src, dst)
    };
    let mut context = TransferContext {
        control: &control,
        id: 1,
        tx: &tx,
        bytes: &mut bytes,
        provider: &provider,
    };
    let error = transfer_staged(
        &f.path("src/file"),
        &f.path("dst/file"),
        JobKind::Move,
        true,
        &mut context,
        false,
        |_, _| Ok(()),
    )
    .unwrap_err();
    assert!(observed.load(Ordering::Relaxed));
    assert!(matches!(error, JobError::CommittedWithRecovery(_)));
    assert_eq!(fs::read(f.path("dst/file")).unwrap(), b"SOURCE");
    assert!(!f.path("src/file").exists());
    let retained = source_stages(&f);
    assert_eq!(retained.len(), 1);
    assert_eq!(fs::read(retained[0].join("captured")).unwrap(), b"SOURCE");
    let record = fs::read_to_string(retained[0].join("recovery.txt")).unwrap();
    assert!(record.contains("destination WAS published"));
    assert!(record.contains(&f.path("dst/file").display().to_string()));
}
#[test]
fn provider_uncertain_private_capture_errors_restore_without_recursive_disposal() {
    for uncertain_name in ["captured", "payload"] {
        let f = Fixture::new();
        fs::write(f.path("src/file"), b"SOURCE").unwrap();
        fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
        let control = JobControl::new();
        let (tx, _rx) = mpsc::channel();
        let mut bytes = 0;
        let injected = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = injected.clone();
        let provider = move |src: &Path, dst: &Path| {
            rename_no_replace(src, dst)?;
            if dst.file_name().is_some_and(|s| s == uncertain_name)
                && !observed.swap(true, Ordering::Relaxed)
            {
                return Err(std::io::Error::other("injected error after private rename"));
            }
            Ok(())
        };
        let mut context = TransferContext {
            control: &control,
            id: 1,
            tx: &tx,
            bytes: &mut bytes,
            provider: &provider,
        };
        assert!(transfer_staged(
            &f.path("src/file"),
            &f.path("dst/file"),
            JobKind::Move,
            true,
            &mut context,
            false,
            |_, _| Ok(())
        )
        .is_err());
        assert!(injected.load(Ordering::Relaxed));
        assert_eq!(fs::read(f.path("src/file")).unwrap(), b"SOURCE");
        assert_eq!(fs::read(f.path("dst/file")).unwrap(), b"PRECIOUS");
        assert_no_stages(&f);
        assert!(source_stages(&f).is_empty());
    }
}
#[test]
fn provider_uncertain_publication_preserves_public_entry_and_backup() {
    let f = Fixture::new();
    fs::write(f.path("src/file"), b"SOURCE").unwrap();
    fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
    let control = JobControl::new();
    let (tx, _rx) = mpsc::channel();
    let mut bytes = 0;
    let destination = f.path("dst/file");
    let provider = move |src: &Path, dst: &Path| {
        rename_no_replace(src, dst)?;
        if dst == destination {
            return Err(std::io::Error::other("injected uncertain publication"));
        }
        Ok(())
    };
    let mut context = TransferContext {
        control: &control,
        id: 1,
        tx: &tx,
        bytes: &mut bytes,
        provider: &provider,
    };
    let error = transfer_staged(
        &f.path("src/file"),
        &f.path("dst/file"),
        JobKind::Move,
        true,
        &mut context,
        false,
        |_, _| Ok(()),
    )
    .unwrap_err();
    assert!(matches!(error, JobError::CommittedWithRecovery(_)));
    assert_eq!(fs::read(f.path("dst/file")).unwrap(), b"SOURCE");
    let retained = stages(&f);
    assert_eq!(retained.len(), 1);
    assert_eq!(fs::read(retained[0].join("backup")).unwrap(), b"PRECIOUS");
}

#[test]
fn skip_only_batch_failure_has_no_dangling_detail_separator() {
    let f = Fixture::new();
    fs::write(f.path("src/file"), b"SOURCE").unwrap();
    fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
    let result = transfer(&f, JobKind::Copy, "file", OverwritePolicy::SkipExisting);
    match result {
        Err(JobError::Io(message)) => assert_eq!(message, "1 of 1 items failed"),
        other => panic!("unexpected outcome: {other:?}"),
    }
    assert_eq!(fs::read(f.path("src/file")).unwrap(), b"SOURCE");
    assert_eq!(fs::read(f.path("dst/file")).unwrap(), b"PRECIOUS");
}

#[cfg(unix)]
#[test]
fn recovery_locator_preserves_non_utf8_paths_and_fresh_process_discovery() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    if let Some(record) = std::env::var_os("IRA_T015_RECOVERY_RECORD") {
        let text = fs::read_to_string(&record).unwrap();
        let encoded = text
            .lines()
            .find_map(|line| line.strip_prefix("source_raw=unix-bytes-hex:"))
            .unwrap();
        let bytes: Vec<u8> = (0..encoded.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16).unwrap())
            .collect();
        let recovered = PathBuf::from(std::ffi::OsString::from_vec(bytes));
        assert_eq!(
            recovered.as_os_str().as_bytes(),
            std::env::var_os("IRA_T015_EXPECTED_SOURCE")
                .unwrap()
                .as_os_str()
                .as_bytes()
        );
        assert_eq!(fs::read(&recovered).unwrap(), b"ACTOR");
        assert_eq!(
            fs::read(Path::new(&record).parent().unwrap().join("captured")).unwrap(),
            b"SOURCE"
        );
        return;
    }
    let f = Fixture::new();
    // This macOS filesystem rejects non-UTF8 entry names. Prove raw
    // encoding roundtrip separately, then exercise newline-safe disk records
    // and actual restart discovery with a supported filesystem name.
    let invalid_path = f
        .path("src")
        .join(std::ffi::OsString::from_vec(vec![b'f', 0xff, b'\n']));
    let encoded = encoded_path(&invalid_path);
    let hex = encoded.strip_prefix("unix-bytes-hex:").unwrap();
    let decoded: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    assert_eq!(decoded, invalid_path.as_os_str().as_bytes());
    let src = f.path("src/file\nliteral source_raw=field");
    let dst = f.path("dst/file");
    fs::write(&src, b"SOURCE").unwrap();
    fs::write(&dst, b"PRECIOUS").unwrap();
    let control = JobControl::new();
    let (tx, _rx) = mpsc::channel();
    let mut bytes = 0;
    let mut context = TransferContext {
        control: &control,
        id: 1,
        tx: &tx,
        bytes: &mut bytes,
        provider: &rename_no_replace,
    };
    let error = transfer_staged(
        &src,
        &dst,
        JobKind::Move,
        true,
        &mut context,
        false,
        |point, path| {
            if point == StagePoint::SourceCaptured {
                let text = fs::read_to_string(path.parent().unwrap().join("recovery.txt")).unwrap();
                assert!(text.contains("before source capture"));
                assert!(text
                    .lines()
                    .any(|line| line.starts_with("destination_stage_raw=unix-bytes-hex:")));
                fs::write(&src, b"ACTOR").unwrap();
                return Err(JobError::Cancelled);
            }
            Ok(())
        },
    )
    .unwrap_err();
    let retained = source_stages(&f);
    assert_eq!(retained.len(), 1);
    let record = retained[0].join("recovery.txt");
    assert!(format!("{error:?}").contains(&retained[0].display().to_string()));
    let test_name = format!(
        "{}::recovery_locator_preserves_non_utf8_paths_and_fresh_process_discovery",
        module_path!().split_once("::").unwrap().1
    );
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test_name, "--nocapture"])
        .env("IRA_T015_RECOVERY_RECORD", &record)
        .env("IRA_T015_EXPECTED_SOURCE", src.as_os_str())
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "child stdout={} stderr={}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(String::from_utf8_lossy(&child.stdout).contains("1 passed"));
}

#[test]
fn cancelled_restore_conflict_remains_in_persistent_final_event() {
    let f = Fixture::new();
    fs::write(f.path("src/file"), b"SOURCE").unwrap();
    fs::write(f.path("src/second"), b"SECOND").unwrap();
    let control = JobControl::new();
    let provider_control = control.clone();
    let source = f.path("src/file");
    let provider = move |from: &Path, to: &Path| {
        rename_no_replace(from, to)?;
        if to.file_name().is_some_and(|name| name == "captured") {
            fs::write(&source, b"ACTOR")?;
            provider_control.request_cancel();
        }
        Ok(())
    };
    let job = Job {
        id: 1,
        kind: JobKind::Move,
        overwrite: OverwritePolicy::AutoRename,
        paths: vec![
            f.path("src/file").to_string_lossy().into_owned(),
            f.path("src/second").to_string_lossy().into_owned(),
        ],
        dest_dir: f.path("dst").to_string_lossy().into_owned(),
        label: String::new(),
        total_bytes: None,
        copied_bytes: 0,
        current: String::new(),
        status: JobStatus::Queued,
        started_at: Instant::now(),
        control,
    };
    let (tx, rx) = mpsc::channel();
    spawn_job_with_provider(&job, tx, Arc::new(provider));
    let final_event = loop {
        let event = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        if matches!(
            event,
            JobEvent::Done { .. } | JobEvent::Failed { .. } | JobEvent::Cancelled { .. }
        ) {
            break event;
        }
    };
    let retained = source_stages(&f);
    assert_eq!(retained.len(), 1);
    assert_eq!(fs::read(retained[0].join("captured")).unwrap(), b"SOURCE");
    assert_eq!(fs::read(f.path("src/file")).unwrap(), b"ACTOR");
    assert_eq!(fs::read(f.path("src/second")).unwrap(), b"SECOND");
    assert!(!f.path("dst/second").exists());
    match final_event {
        JobEvent::Failed { error, .. } => {
            assert!(error.contains(&retained[0].display().to_string()));
            assert!(error.contains("cancelled"));
            assert!(!error.contains("WAS published"));
        }
        other => panic!("recovery must persist in final status: {other:?}"),
    }
}
#[test]
fn later_committed_recovery_includes_prior_failed_item_recovery_path() {
    let f = Fixture::new();
    fs::write(f.path("src/file"), b"SOURCE").unwrap();
    fs::write(f.path("src/second"), b"SECOND").unwrap();
    fs::write(f.path("dst/file"), b"PRECIOUS").unwrap();
    fs::write(f.path("dst/second"), b"OLD SECOND").unwrap();
    let source = f.path("src/file");
    let destination = f.path("dst/file");
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let capture_count = count.clone();
    let provider = move |from: &Path, to: &Path| {
        if from == destination && to.file_name().is_some_and(|name| name == "backup") {
            return Err(std::io::Error::other(
                "injected first destination capture failure",
            ));
        }
        if from.file_name().is_some_and(|name| name == "captured")
            && to.file_name().is_some_and(|name| name == "payload")
            && capture_count.load(Ordering::Relaxed) == 2
        {
            return Err(std::io::Error::from(ErrorKind::CrossesDevices));
        }
        rename_no_replace(from, to)?;
        if to.file_name().is_some_and(|name| name == "captured")
            && capture_count.fetch_add(1, Ordering::Relaxed) == 0
        {
            fs::write(&source, b"ACTOR")?;
        }
        Ok(())
    };
    let control = JobControl::new();
    let (tx, _rx) = mpsc::channel();
    let error = run_batch_with_provider(
        1,
        JobKind::Move,
        &[
            f.path("src/file").to_string_lossy().into_owned(),
            f.path("src/second").to_string_lossy().into_owned(),
        ],
        &f.path("dst"),
        OverwritePolicy::Overwrite,
        &control,
        &tx,
        &provider,
    )
    .unwrap_err();
    assert!(matches!(error, JobError::CommittedWithRecovery(_)));
    let old = stages(&f);
    let copied = source_stages(&f);
    assert_eq!(old.len(), 1);
    assert_eq!(copied.len(), 1);
    assert_eq!(fs::read(old[0].join("payload")).unwrap(), b"SOURCE");
    assert_eq!(fs::read(copied[0].join("captured")).unwrap(), b"SECOND");
    assert_eq!(fs::read(f.path("src/file")).unwrap(), b"ACTOR");
    assert_eq!(fs::read(f.path("dst/file")).unwrap(), b"PRECIOUS");
    assert_eq!(fs::read(f.path("dst/second")).unwrap(), b"SECOND");
    let diagnostic = format!("{error:?}");
    for path in [&old[0], &copied[0]] {
        assert!(
            diagnostic.contains(&path.display().to_string()),
            "missing {path:?}: {diagnostic}"
        );
    }
    assert!(diagnostic.contains("WAS published"));
}
