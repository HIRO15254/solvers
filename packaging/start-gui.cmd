@echo off
cd /d "%~dp0" || exit /b 1
solversd --open --runs runs
rem Keep a double-clicked window open so a startup error stays readable.
if errorlevel 1 pause
