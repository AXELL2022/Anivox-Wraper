# Изоляция нативных Windows CI-тестов

Дата: 2026-10-19.

Причина падения: на self-hosted runner уже работал пользовательский debug Anivox с com.yasin.anivox. Второй процесс той же application identity завершался через single-instance до создания окна; это не было падением иконки.

Исправление:
- После обычной подписанной сборки prepare-native-ci.ps1 сохраняет production EXE, собирает отдельный EXE без установщиков с идентификатором com.yasin.anivox.ci-native и копирует его во временный каталог job.
- Production EXE восстанавливается в finally, его SHA-256 проверяется. Обычные установщики не пересобираются с тестовым идентификатором.
- Оба нативных теста используют только тестовый EXE. Проверка single-instance запрещает только уже работающий экземпляр того же тестируемого файла; пользовательские процессы не завершаются.

Локальная проверка при работающем пользовательском Anivox:
- Подготовка изолированного EXE: passed.
- SHA-256 production EXE до/после совпал.
- windows_taskbar_icon.ps1: ICON_SMALL и ICON_BIG passed.
- windows_single_instance.ps1: visible, minimized, hidden passed, включая фокус окна.
- Пользовательский процесс остался работать с прежним PID.
- YAML workflow и синтаксис PowerShell: passed.
