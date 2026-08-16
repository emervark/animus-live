@echo off
title M0-1 stress, 50 nukku
echo.
echo   VAATA: kas koik 50 nukku on nahtaval, ruudustikus?
echo          Kas nad liiguvad IGAUKS ISE, mitte kaik uhtemoodi?
echo.
"%~dp0..\target\release\m0-1-skinned-mesh.exe" --stress
