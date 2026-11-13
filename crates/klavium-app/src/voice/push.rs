//! Конечный автомат push-to-talk.
//!
//! Удержание клавиши диктовки — это две точки входа, которые приходят
//! из хука: нажатие (старт) и отпускание (стоп). Windows присылает
//! отпускание даже если фокус уже уехал в другое окно, но порядок событий
//! может быть любым (автоповтор, отпускание без нажатия после перезапуска
//! хука), поэтому переходы собраны в маленький автомат с тестами.

/// Что сделать с диктовкой по событию клавиши.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushAction {
    /// Начать запись.
    Start,
    /// Остановить запись и распознать.
    Stop,
    /// Ничего не делать.
    Ignore,
}

/// Состояние удержания клавиши диктовки.
#[derive(Debug, Default, Clone, Copy)]
pub struct PushToTalk {
    active: bool,
}

impl PushToTalk {
    /// Новый автомат (клавиша не нажата).
    #[must_use]
    pub const fn new() -> Self {
        Self { active: false }
    }

    /// Идёт ли запись по удержанию.
    #[must_use]
    pub const fn is_active(self) -> bool {
        self.active
    }

    /// Клавиша нажата.
    #[must_use]
    pub const fn on_down(&mut self) -> PushAction {
        if self.active {
            // Автоповтор: уже пишем.
            PushAction::Ignore
        } else {
            self.active = true;
            PushAction::Start
        }
    }

    /// Клавиша отпущена.
    #[must_use]
    pub const fn on_up(&mut self) -> PushAction {
        if self.active {
            self.active = false;
            PushAction::Stop
        } else {
            PushAction::Ignore
        }
    }

    /// Сбросить состояние (смена режима, выключение диктовки, потеря хука).
    ///
    /// Возвращает [`PushAction::Stop`], если запись шла: её нужно завершить,
    /// иначе сессия останется висеть без отпускания клавиши.
    #[must_use]
    pub const fn reset(&mut self) -> PushAction {
        if self.active {
            self.active = false;
            PushAction::Stop
        } else {
            PushAction::Ignore
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_starts_once() {
        let mut push = PushToTalk::new();
        assert_eq!(push.on_down(), PushAction::Start);
        assert_eq!(
            push.on_down(),
            PushAction::Ignore,
            "автоповтор не перезапускает"
        );
        assert!(push.is_active());
    }

    #[test]
    fn up_stops_only_after_start() {
        let mut push = PushToTalk::new();
        assert_eq!(push.on_up(), PushAction::Ignore, "отпускание без нажатия");
        let _ = push.on_down();
        assert_eq!(push.on_up(), PushAction::Stop);
        assert!(!push.is_active());
        assert_eq!(push.on_up(), PushAction::Ignore, "повторное отпускание");
    }

    #[test]
    fn full_cycle_can_repeat() {
        let mut push = PushToTalk::new();
        for _ in 0..3 {
            assert_eq!(push.on_down(), PushAction::Start);
            assert_eq!(push.on_up(), PushAction::Stop);
        }
    }

    #[test]
    fn reset_stops_active_recording() {
        let mut push = PushToTalk::new();
        let _ = push.on_down();
        assert_eq!(push.reset(), PushAction::Stop);
        assert_eq!(push.reset(), PushAction::Ignore);
    }
}
