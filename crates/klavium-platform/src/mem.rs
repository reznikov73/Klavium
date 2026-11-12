//! Память процесса: возврат системе неиспользуемых страниц.

use windows_sys::Win32::System::ProcessStatus::EmptyWorkingSet;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

/// Отдать системе неиспользуемые страницы рабочего набора.
///
/// Косметика для Диспетчера задач: после тяжёлых операций (старт, выгрузка
/// модели распознавания) RSS процесса отражает реальное потребление, а не
/// «запас», оставленный аллокатором. Вызывать не чаще раза в минуту.
pub fn trim_working_set() {
    unsafe {
        EmptyWorkingSet(GetCurrentProcess());
    }
}
