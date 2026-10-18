@echo off
setlocal
cd /d "%~dp0"
rem Builds "release", the portable copy of ReTzar: ReTzar.exe and ReTzar.dat,
rem the sealed pack of what ReTzar reads from an installed Tzar of any
rem edition (CD, GOG or Steam; with the American CD also its campaign and
rem movies, with FX's Spanish edition its voices, screens and intro), in
rem modern formats and checked file by file.
rem A ReTzar.dat already in "release", such as the release zip's with every
rem edition, is kept and only ReTzar.exe is rebuilt, unless a folder is
rem given. Settings, progress and saves already in "release" stay.
rem The original game: the folder given to this script, TZAR_DATA_ROOT, or
rem the folder that holds the project. The American CD: USA\CD\SETUP\Tzar
rem inside it, or TZAR_USA_ROOT. The Spanish edition: Anthology inside it,
rem or TZAR_SPANISH_ROOT.
set "TZAR=%~dp0.."
if defined TZAR_DATA_ROOT set "TZAR=%TZAR_DATA_ROOT%"
if not "%~1"=="" set "TZAR=%~1"
set "USA=%TZAR%\USA\CD\SETUP\Tzar"
if defined TZAR_USA_ROOT set "USA=%TZAR_USA_ROOT%"
set "SPANISH=%TZAR%\Anthology"
if defined TZAR_SPANISH_ROOT set "SPANISH=%TZAR_SPANISH_ROOT%"
set "DEST=%~dp0release"

if "%~1"=="" if exist "%DEST%\ReTzar.dat" goto exe_only
if not exist "%TZAR%\Maps\" goto no_game
if exist "%TZAR%\Packs\" goto full
if exist "%TZAR%\IMAGES\" goto full
:no_game
if exist "%DEST%\ReTzar.dat" goto exe_only
echo Tzar not found in "%TZAR%".
echo Give the folder where it is installed, for example:
echo build-release.cmd "C:\GOG Games\Tzar"
goto failed

:full
set "PACK=yes"
set EXTRA=
if exist "%USA%\Packs\movies.wdt" set EXTRA=--americano "%USA%"
if exist "%SPANISH%\Campaign\" set EXTRA=%EXTRA% --spagnolo "%SPANISH%"
goto build

:exe_only
echo The ReTzar.dat already in "%DEST%" is kept, only ReTzar.exe is rebuilt.
set "PACK=none"

:build
echo Building the optimized version...
cargo build --release --locked --bin retzar --bin retzar-tools
if errorlevel 1 goto failed

if not exist "%DEST%" mkdir "%DEST%"
if "%PACK%"=="none" goto copy_exe
echo Building the data pack...
"target\release\retzar-tools.exe" --source "%TZAR%" %EXTRA% --crea-pacchetto "%DEST%\ReTzar.dat"
if errorlevel 1 goto failed

:copy_exe
copy /y "target\release\retzar.exe" "%DEST%\ReTzar.exe" >nul
if errorlevel 1 goto failed
rem The first time, the settings and progress of the project folder.
for %%f in (settings.json setup-options.json random-options.json) do (
    if exist "%%f" if not exist "%DEST%\%%f" copy "%%f" "%DEST%\%%f" >nul
)
if not exist "%DEST%\campaign-progress.rtzp" if not exist "%DEST%\campaign-progress.json" (
    if exist campaign-progress.rtzp (
        copy campaign-progress.rtzp "%DEST%\campaign-progress.rtzp" >nul
    ) else if exist campaign-progress.json (
        copy campaign-progress.json "%DEST%\campaign-progress.json" >nul
    )
)

echo.
echo Ready: %DEST%
echo Play with ReTzar.exe; the folder can be moved or copied anywhere.
pause
exit /b 0

:failed
echo.
echo The release could not be built.
pause
exit /b 1
