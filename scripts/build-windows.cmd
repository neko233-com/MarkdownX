@echo off
REM =============================================================================
REM MarkdownX — Windows x64 build (Go + MyGo native UI)
REM Product: dist\windows-amd64\MarkdownX.exe and markdownx-setup-<version>-amd64.exe
REM =============================================================================
setlocal EnableDelayedExpansion
cd /d "%~dp0.."

echo.
echo ==============================================
echo  MarkdownX Windows x64 build (Go / MyGo)
echo ==============================================

set CGO_ENABLED=0

echo [1/3] go vet ...
go vet ./...
if errorlevel 1 exit /b 1

echo [2/3] go test ...
go test ./...
if errorlevel 1 exit /b 1

echo [3/3] go tool mygo build ...
go tool mygo build -platform windows/amd64 -o dist
if errorlevel 1 exit /b 1

if not exist dist\cli mkdir dist\cli
go build -trimpath -ldflags "-s -w" -o dist\cli\markdownx-server.exe .\cmd\markdownx-server
if errorlevel 1 exit /b 1
go build -trimpath -ldflags "-s -w" -o dist\cli\mdx-convert.exe .\cmd\markdownx-convert
if errorlevel 1 exit /b 1
go build -trimpath -ldflags "-s -w" -o dist\cli\mdx-enc.exe .\cmd\mdx-enc
if errorlevel 1 exit /b 1

echo.
echo OK  dist\windows-amd64\
dir /b dist\windows-amd64
echo.
echo OK  dist\cli\
dir /b dist\cli
echo ==============================================
endlocal
