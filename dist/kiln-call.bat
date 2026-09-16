@echo off
rem Kiln-noGUI-CLI ??(? artboard ??????)
rem ??: kiln-call.bat <source> <output> [format] [scale]
set EXE=%~dp0Kiln-noGUI-CLI.exe
if not exist "%EXE%" ( echo ??? %EXE% & exit /b 1 )
"%EXE%" export --source "%~1" --output "%~2" %~3 %~4
exit /b %ERRORLEVEL%
