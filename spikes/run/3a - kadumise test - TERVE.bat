@echo off
title 3a TERVE (bounds fix sees)
echo.
echo   TERVE VERSIOON. Ava korvale ka fail "3b - kadumise test - KATKINE".
echo.
echo   Kaamera poorab nii, et mesh kaib kaadri servast labi.
echo   VAATA: siin peab mesh kaduma alles siis, kui ta on PARISELT
echo          ekraanilt valjas.
echo.
"%~dp0..\target\release\m0-1-skinned-mesh.exe" --edge-sweep --amplitude 2.0
