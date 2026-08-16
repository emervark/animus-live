@echo off
title 3b KATKINE (--no-bounds-fix)
echo.
echo   KATKINE VERSIOON. Ava korvale ka fail "3a - kadumise test - TERVE".
echo.
echo   VAATA: kas siin kaob mesh serva juures VAREM ara - hupatab
echo          nahtamatuks, kuigi tukk temast on veel ekraanil?
echo          Kui jah, siis bounds-parandus on parisel vajalik.
echo.
"%~dp0..\target\release\m0-1-skinned-mesh.exe" --edge-sweep --amplitude 2.0 --no-bounds-fix
