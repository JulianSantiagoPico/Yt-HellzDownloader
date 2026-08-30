# Avisos de terceros

## yt-dlp

El ejecutable oficial de yt-dlp se distribuye bajo Unlicense, ISC, MIT y las
licencias de sus componentes empaquetados. Antes de publicar una distribución,
debe incluirse también el archivo `THIRD_PARTY_LICENSES.txt` correspondiente a
la versión exacta descargada desde su release oficial.

Fuente: https://github.com/yt-dlp/yt-dlp

## FFmpeg

FFmpeg se distribuye conforme a LGPL 2.1 o posterior, o GPL 2 o posterior según
las opciones del build. El script usa el build `release-essentials` de gyan.dev,
proveedor enlazado por el sitio oficial de FFmpeg. Antes de distribuir, se debe
confirmar mediante `ffmpeg -L` la configuración y adjuntar el texto de licencia
y la oferta/código fuente exigidos por ese build.

Fuentes:
- https://ffmpeg.org/legal.html
- https://www.gyan.dev/ffmpeg/builds/

La revisión legal final y los textos íntegros son un requisito de salida de la
Fase 0; este documento no sustituye asesoramiento legal.
