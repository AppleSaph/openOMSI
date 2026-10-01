@echo off
setlocal
rem Keep this personal executable from being replaced by an official release.
set "OMSI_NO_UPDATE=1"
set "OMSI_PERSONAL_EXE=%USERPROFILE%\Desktop\openOmsiPersonalBuild\release\openomsi.exe"
if not exist "%OMSI_PERSONAL_EXE%" (
    echo Please build the personal version first. See docs\PERSONAL-VR.md.
    pause
    exit /b 1
)
start "" "%OMSI_PERSONAL_EXE%"
