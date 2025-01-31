use arcstr::ArcStr;
use dashmap::DashSet;
use notify::{
  event::ModifyKind, Config, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher,
};
use rolldown_common::{
  BundleEndEventData, BundleEventKind, WatcherChange, WatcherChangeKind, WatcherEvent,
  WatcherEventData,
};
use rolldown_error::DiagnosticOptions;
use rolldown_utils::pattern_filter;
use std::{
  path::Path,
  sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{channel, Receiver, Sender},
    Arc,
  },
  time::Instant,
};
use sugar_path::SugarPath;
use tokio::sync::Mutex;

use crate::Bundler;

use anyhow::Result;

use super::emitter::{SharedWatcherEmitter, WatcherEmitter};

enum WatcherChannelMsg {
  NotifyEvent(notify::Result<notify::Event>),
  Close,
}

pub struct Watcher {
  pub emitter: SharedWatcherEmitter,
  bundler: Arc<Mutex<Bundler>>,
  inner: Arc<Mutex<RecommendedWatcher>>,
  running: AtomicBool,
  rerun: AtomicBool,
  watch_files: DashSet<ArcStr>,
  tx: Arc<Sender<WatcherChannelMsg>>,
  rx: Arc<Mutex<Receiver<WatcherChannelMsg>>>,
}

impl Watcher {
  pub fn new(bundler: Arc<Mutex<Bundler>>) -> Result<Self> {
    let (tx, rx) = channel();
    let tx = Arc::new(tx);
    let cloned_tx = Arc::clone(&tx);
    let watch_option = {
      let config = Config::default();
      let bundler_guard = bundler.try_lock().expect("Failed to lock the bundler. ");
      if let Some(notify) = &bundler_guard.options.watch.notify {
        if let Some(poll_interval) = notify.poll_interval {
          config.with_poll_interval(poll_interval);
        }
        config.with_compare_contents(notify.compare_contents);
      }
      config
    };
    let inner = RecommendedWatcher::new(
      move |res| {
        if let Err(e) = tx.send(WatcherChannelMsg::NotifyEvent(res)) {
          eprintln!("send watch event error {e:?}");
        };
      },
      watch_option,
    )?;

    Ok(Self {
      emitter: Arc::new(WatcherEmitter::new()),
      bundler,
      inner: Arc::new(Mutex::new(inner)),
      running: AtomicBool::default(),
      watch_files: DashSet::default(),
      rerun: AtomicBool::default(),
      rx: Arc::new(Mutex::new(rx)),
      tx: cloned_tx,
    })
  }

  pub fn invalidate(&self) {
    if self.running.load(Ordering::Relaxed) {
      self.rerun.store(true, Ordering::Relaxed);
      return;
    }
    if self.rerun.load(Ordering::Relaxed) {
      return;
    }

    let future = async move {
      self.rerun.store(false, Ordering::Relaxed);
      let _ = self.run().await;
    };

    #[cfg(target_family = "wasm")]
    {
      futures::executor::block_on(future);
    }
    #[cfg(not(target_family = "wasm"))]
    {
      tokio::task::block_in_place(move || {
        tokio::runtime::Handle::current().block_on(future);
      });
    }
  }

  pub async fn run(&self) -> Result<()> {
    let start_time = Instant::now();
    let mut bundler = self.bundler.lock().await;
    self.emitter.emit(WatcherEvent::ReStart, WatcherEventData::default()).await?;

    self.running.store(true, Ordering::Relaxed);
    self.emitter.emit(WatcherEvent::Event, BundleEventKind::Start.into()).await?;

    self.emitter.emit(WatcherEvent::Event, BundleEventKind::BundleStart.into()).await?;

    bundler.plugin_driver.clear();

    let mut output = {
      if bundler.options.watch.skip_write {
        // TODO Here should be call scan
        bundler.generate().await?
      } else {
        bundler.write().await?
      }
    };
    let mut inner = self.inner.lock().await;
    for file in &output.watch_files {
      // we should skip the file that is already watched, here here some reasons:
      // - The watching files has a ms level overhead.
      // - Watching the same files multiple times will cost more overhead.
      // TODO: tracking https://github.com/notify-rs/notify/issues/653
      if self.watch_files.contains(file) {
        continue;
      }
      let path = Path::new(file.as_str());
      if path.exists() {
        let normalized_path = path.relative(&bundler.options.cwd);
        let normalized_id = normalized_path.to_string_lossy();
        if pattern_filter::filter(
          bundler.options.watch.exclude.as_deref(),
          bundler.options.watch.include.as_deref(),
          file.as_str(),
          &normalized_id,
        )
        .inner()
        {
          inner.watch(path, RecursiveMode::Recursive)?;
          self.watch_files.insert(file.clone());
        }
      }
    }
    // The inner mutex should be dropped to avoid deadlock with bundler lock at `Watcher::close`
    std::mem::drop(inner);

    if output.errors.is_empty() {
      self
        .emitter
        .emit(
          WatcherEvent::Event,
          BundleEventKind::BundleEnd(BundleEndEventData {
            output: bundler.options.cwd.join(&bundler.options.dir).to_string_lossy().to_string(),
            duration: start_time.elapsed().as_millis().to_string(),
          })
          .into(),
        )
        .await?;
    } else {
      self
        .emitter
        .emit(
          WatcherEvent::Event,
          BundleEventKind::Error(
            output
              .errors
              .remove(0)
              .into_diagnostic_with(&DiagnosticOptions { cwd: bundler.options.cwd.clone() })
              .to_color_string(),
          )
          .into(),
        )
        .await?;
    }

    self.running.store(false, Ordering::Relaxed);
    self.emitter.emit(WatcherEvent::Event, BundleEventKind::End.into()).await?;

    Ok(())
  }

  pub async fn close(&self) -> anyhow::Result<()> {
    // close channel
    self.tx.send(WatcherChannelMsg::Close)?;
    // stop watching files
    // TODO the notify watcher should be dropped, because the stop method is private
    let mut inner = self.inner.lock().await;
    for path in self.watch_files.iter() {
      inner.unwatch(Path::new(path.as_str()))?;
    }
    // The inner mutex should be dropped to avoid deadlock with bundler lock at `Watcher::run`
    std::mem::drop(inner);
    // emit close event
    self.emitter.emit(WatcherEvent::Close, WatcherEventData::default()).await?;
    // call close watcher hook
    let bundler = self.bundler.lock().await;
    bundler.plugin_driver.close_watcher().await?;

    Ok(())
  }
}

pub async fn on_change(watcher: &Arc<Watcher>, path: &str, kind: WatcherChangeKind) {
  let _ = watcher
    .emitter
    .emit(WatcherEvent::Change, WatcherChange { path: path.into(), kind }.into())
    .await
    .map_err(|e| eprintln!("Rolldown internal error: {e:?}"));
  let bundler = watcher.bundler.lock().await;
  let _ = bundler
    .plugin_driver
    .watch_change(path, kind)
    .await
    .map_err(|e| eprintln!("Rolldown internal error: {e:?}"));
}

pub fn wait_for_change(watcher: Arc<Watcher>) {
  let future = async move {
    let mut run = true;
    while run {
      let rx = watcher.rx.lock().await;
      match rx.recv() {
        Ok(msg) => match msg {
          WatcherChannelMsg::NotifyEvent(event) => match event {
            Ok(event) => {
              for path in event.paths {
                let id = path.to_string_lossy();
                match event.kind {
                  notify::EventKind::Create(_) => {
                    on_change(&watcher, id.as_ref(), WatcherChangeKind::Create).await;
                  }
                  notify::EventKind::Modify(
                    ModifyKind::Data(_) | ModifyKind::Any, /* windows*/
                  ) => {
                    on_change(&watcher, id.as_ref(), WatcherChangeKind::Update).await;
                    watcher.invalidate();
                  }
                  notify::EventKind::Remove(_) => {
                    on_change(&watcher, id.as_ref(), WatcherChangeKind::Delete).await;
                  }
                  _ => {}
                }
              }
            }
            Err(e) => eprintln!("notify error: {e:?}"),
          },
          WatcherChannelMsg::Close => run = false,
        },
        Err(e) => {
          eprintln!("watcher receiver error: {e:?}");
        }
      }
    }
  };

  #[cfg(target_family = "wasm")]
  {
    let handle = tokio::runtime::Handle::current();
    // could not block_on/spawn the main thread in WASI
    std::thread::spawn(move || {
      handle.spawn(future);
    });
  }
  #[cfg(not(target_family = "wasm"))]
  tokio::spawn(future);
}
