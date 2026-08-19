@echo off
cd /d "%~dp0"
echo Compilando Microverse en modo release (mucho mas fluido)...
echo La primera vez puede tardar 1-2 minutos.
echo.

cargo run --release 1> run_out.txt 2> run_err.txt
set EXITCODE=%ERRORLEVEL%

if %EXITCODE% equ 0 (
  echo.
  echo Cerrado correctamente.
  goto :eof
)

REM Ctrl+C / cierre de ventana = 0xC000013A — no es un fallo del juego.
if %EXITCODE% equ -1073741510 (
  echo.
  echo Sesion terminada ^(Ctrl+C o ventana cerrada^).
  goto :eof
)
if %EXITCODE% equ 3221225786 (
  echo.
  echo Sesion terminada ^(Ctrl+C o ventana cerrada^).
  goto :eof
)

echo.
echo --- Fallo al ejecutar ^(codigo %EXITCODE%^) — ver run_err.txt ---
type run_err.txt
echo.
pause
