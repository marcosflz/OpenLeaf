//! Bus de eventos genérico con `async_channel` (M1-A0).
//!
//! # Desviación ratificada del borrador (`INTERFACES.md` §2)
//!
//! El borrador describe `pub enum AppEvent { Compile(..), Pdf(..), Chat(..), Fs(..), Ui(..) }`
//! dentro de `lsx-core`. Eso obligaría a `lsx-core` a conocer los tipos de
//! dominio de los demás crates (o a duplicarlos), rompiendo la ley de
//! dependencias del §1. El orchestrator ratificó la alternativa: aquí vive un
//! **bus genérico** (`EventBus<T>`) y cada capa define su propio enum de
//! eventos (UI en `lsx-ui`, chat en `lsx-chat`…), publicado por el canal.
//!
//! Semántica:
//!
//! - `subscribe()` devuelve un [`Receiver`] propio (fan-out); cada suscriptor
//!   recibe **clones** del evento si `T: Clone`.
//! - `publish()` nunca bloquea: un suscriptor cuyos mensajes no se consumen
//!   no frena al resto (canales ilimitados; el *batching* es responsabilidad
//!   del consumidor, p. ej. los reducers de panel de `lsx-ui`).
//! - Los suscriptores cuyo `Receiver` se soltó se expurgan en el siguiente
//!   `publish`/`subscriber_count`.
//!
//! `lsx-core` sigue sin conocer tokio/GPUI: `async_channel` es un crate
//! independiente del runtime.

use std::sync::Mutex;

use async_channel::{Receiver, Sender, unbounded};

/// Bus de publicación/suscripción tipado y multi-consumidor.
pub struct EventBus<T> {
    subscribers: Mutex<Vec<Sender<T>>>,
}

impl<T> EventBus<T> {
    /// Crea un bus sin suscriptores.
    pub fn new() -> Self {
        Self {
            subscribers: Mutex::new(Vec::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Sender<T>>> {
        // El bus no hace nada dentro del lock que pueda envenenarlo, pero si un
        // test paniquea mientras publica, seguimos siendo utilizables.
        self.subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Número de suscriptores vivos (expurga los canales cerrados).
    pub fn subscriber_count(&self) -> usize {
        let mut subscribers = self.lock();
        subscribers.retain(|tx| !tx.is_closed());
        subscribers.len()
    }
}

impl<T: Clone + Send + 'static> EventBus<T> {
    /// Suscribe un receptor nuevo. Los eventos publicados antes de suscribirse
    /// no se entregan (sin historial).
    pub fn subscribe(&self) -> Receiver<T> {
        let (tx, rx) = unbounded();
        self.lock().push(tx);
        rx
    }

    /// Publica `event` a todos los suscriptores vivos.
    ///
    /// Devuelve cuántos receptores lo recibieron. Un receptor cerrado se
    /// expurga y no cuenta.
    pub fn publish(&self, event: T) -> usize {
        let mut subscribers = self.lock();
        subscribers.retain(|tx| !tx.is_closed());
        let mut delivered = 0;
        for tx in subscribers.iter() {
            if tx.try_send(event.clone()).is_ok() {
                delivered += 1;
            }
        }
        delivered
    }

    /// `true` si no hay suscriptores vivos.
    pub fn is_empty(&self) -> bool {
        self.subscriber_count() == 0
    }
}

impl<T> Default for EventBus<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> std::fmt::Debug for EventBus<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBus")
            .field("subscribers", &self.subscriber_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_published_event_reaches_every_subscriber_once() {
        let bus = EventBus::new();
        let first = bus.subscribe();
        let second = bus.subscribe();

        assert_eq!(bus.publish("hola".to_owned()), 2);
        assert_eq!(first.try_recv().expect("primero"), "hola");
        assert_eq!(second.try_recv().expect("segundo"), "hola");
        assert!(first.try_recv().is_err(), "sin duplicados");
        assert!(second.try_recv().is_err(), "sin duplicados");
    }

    #[test]
    fn events_published_before_subscribing_are_not_replayed() {
        let bus = EventBus::new();
        assert_eq!(bus.publish(1u32), 0);
        let rx = bus.subscribe();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_dropped_receiver_is_pruned_on_the_next_publish() {
        let bus = EventBus::new();
        let alive = bus.subscribe();
        {
            let _dropped = bus.subscribe();
        }
        assert_eq!(bus.subscriber_count(), 1);
        assert_eq!(bus.publish(7u32), 1);
        assert_eq!(alive.try_recv().expect("vivo"), 7);
    }

    #[test]
    fn a_slow_subscriber_does_not_block_the_others() {
        let bus = EventBus::new();
        let idle = bus.subscribe();
        let _unused = bus.subscribe();
        // Aunque nadie lea `_unused`, el evento llega a los dos y no bloquea:
        for value in 0..1_000u32 {
            bus.publish(value);
        }
        let received: Vec<u32> = std::iter::from_fn(|| idle.try_recv().ok()).collect();
        assert_eq!(received.len(), 1_000);
        assert_eq!(received.last(), Some(&999));
    }

    #[test]
    fn subscriber_count_and_is_empty_track_liveness() {
        let bus: EventBus<u8> = EventBus::new();
        assert!(bus.is_empty());
        assert_eq!(bus.subscriber_count(), 0);
        let rx = bus.subscribe();
        assert!(!bus.is_empty());
        assert_eq!(bus.subscriber_count(), 1);
        drop(rx);
        assert!(bus.is_empty());
    }

    #[test]
    fn two_buses_are_independent() {
        let first: EventBus<u8> = EventBus::new();
        let second: EventBus<u8> = EventBus::new();
        let rx = first.subscribe();
        second.publish(1);
        assert!(rx.try_recv().is_err());
        first.publish(2);
        assert_eq!(rx.try_recv().expect("evento"), 2);
    }

    #[test]
    fn blocking_receivers_work_from_other_threads() {
        let bus = std::sync::Arc::new(EventBus::new());
        let rx = bus.subscribe();
        let handle = std::thread::spawn(move || rx.recv_blocking().expect("evento"));
        // Sin suscriptores durmiendo: el publish entrega al canal.
        assert_eq!(bus.publish("desde hilo".to_owned()), 1);
        assert_eq!(handle.join().expect("join"), "desde hilo");
    }
}
