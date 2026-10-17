//! Мост «блокирующая задача → главный поток GTK».
//!
//! Ядро (`linuxprinterg-core`) синхронно вызывает внешние утилиты. Здесь эти
//! вызовы переносятся в `std::thread`, а результат доставляется в главный
//! цикл GTK через `async-channel` + `glib::MainContext::spawn_local`.
//! UI никогда не блокируется.

use linuxprinterg_core::Result;

/// Запустить блокирующую задачу в фоне; `on_done` вызывается в главном потоке.
pub fn run_async<T, F, C>(task: F, on_done: C)
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
    C: FnOnce(Result<T>) + 'static,
{
    let (tx, rx) = async_channel::bounded::<Result<T>>(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(task());
    });
    glib::MainContext::default().spawn_local(async move {
        if let Ok(result) = rx.recv().await {
            on_done(result);
        }
    });
}

/// То же, но с потоком прогресса: `on_progress` вызывается в главном потоке.
pub fn run_with_progress<T, P, F, PD, C>(task: F, on_progress: PD, on_done: C)
where
    T: Send + 'static,
    P: Send + 'static,
    F: FnOnce(async_channel::Sender<P>) -> Result<T> + Send + 'static,
    PD: FnMut(P) + 'static,
    C: FnOnce(Result<T>) + 'static,
{
    let (ptx, prx) = async_channel::unbounded::<P>();
    let (tx, rx) = async_channel::bounded::<Result<T>>(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(task(ptx));
    });
    glib::MainContext::default().spawn_local(async move {
        let mut on_progress = on_progress;
        // Канал прогресса закрывается, когда задача завершается.
        while let Ok(p) = prx.recv().await {
            on_progress(p);
        }
        if let Ok(result) = rx.recv().await {
            on_done(result);
        }
    });
}
