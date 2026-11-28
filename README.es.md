[English](README.md) · **Español**

# VIctory-RTX-Mc3yohan (Capa de Compatibilidad Xbox Series X|S para NVIDIA RTX)

[![CI](https://github.com/mc3yohan-lab/VIctory-RTX-Mc3yohan/actions/workflows/ci.yml/badge.svg?branch=nvidia-rtx4090)](https://github.com/mc3yohan-lab/VIctory-RTX-Mc3yohan/actions/workflows/ci.yml)

> Mantenido por **Mc3yohan** ([github.com/Mc3yohan](https://github.com/Mc3yohan)) · Licencia **GPL-3.0-or-later**

Fork enfocado en GPUs **NVIDIA RTX (Wave32)** de [Marvisak/VIctory](https://github.com/Marvisak/VIctory), construido sobre [avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX) e integrando correcciones verificadas de la comunidad. Traduce llamadas **D3D12.X** y shaders **GFX10 (RDNA2)** de consola a **Direct3D 12 Shader Model 6.6** en Windows 10/11 x64, aprovechando ejecución nativa x86-64 en tu CPU y compilación Wave32 con FP16 nativo (`SM 6.2`) y 48 KiB de memoria compartida en tarjetas NVIDIA.

---

## Aviso Legal, Derechos de Autor y Política de Activos

> **Lee esto antes de usar o contribuir a este repositorio.**

1. **Cero archivos de videojuegos ni contenido protegido:** Este repositorio **no contiene** ejecutables, librerías propietarias, shaders, texturas, modelos, audio, videos, claves criptográficas ni capturas de pantalla de ningún videojuego. Debes aportar una copia legal propia almacenada **fuera** de esta carpeta (configurada en `victory.toml` o desde el launcher).
2. **Propósito estrictamente técnico e interoperabilidad:** Este software es una capa de compatibilidad e investigación técnica en el espíritu de Wine/Proton. No elude sistemas DRM ni medidas tecnológicas de protección; únicamente traduce APIs de consola y bytecode de shaders sobre archivos que el usuario ya posee localmente.
3. **Sin afiliación ni respaldo:** Proyecto independiente, **no afiliado, patrocinado ni respaldado** por Microsoft, Xbox, NVIDIA, AMD ni por ningún desarrollador o editor de videojuegos. Todas las marcas pertenecen a sus respectivos propietarios.
4. **Control automático en cada commit (`cargo xtask policy`):** El árbol está regido por una lista blanca estricta ([docs/asset-policy.md](docs/asset-policy.md)) que bloquea binarios, imágenes y datos embebidos.
   - *Excepción documentada heredada de upstream:* `crates/victory-formats/src/bcpack-tables.bin` (~78 KB) contiene las tablas constantes del formato de compresión BCPack; la licencia GPL-3.0 del repositorio no cubre dicho archivo (consulta [docs/asset-policy.md](docs/asset-policy.md#the-one-exception)).
5. **Solo código fuente:** No distribuimos copias de juegos ni arte propietario. Si el launcher muestra portada, la lee únicamente desde tu carpeta local en tiempo de ejecución.

---

## Requisitos del Sistema

| Componente | Mínimo recomendado | Hardware de referencia (pruebas) |
|---|---|---|
| **Sistema Operativo** | Windows 10 / 11 x64 (actualizado) | Windows 11 x64 |
| **Procesador (CPU)** | x86-64 con soporte **AVX2** (6+ núcleos) | Intel Core i9-12900KF (24 hilos) |
| **Tarjeta Gráfica (GPU)** | NVIDIA RTX serie 30/40/50 (D3D12 SM 6.6 + Mesh Shaders, 8 GB+ VRAM) | NVIDIA GeForce RTX 4090 (24 GB GDDR6X, Resizable BAR) |
| **Memoria RAM** | 16 GB (activa perfil automático `ram_profile`) / 32 GB recomendado | 64 GB DDR5 |
| **Espacio en disco** | ~1.5–2 GB libres para caché limpia de shaders (`~975 MB`) y pipelines (`~255 MB`) | NVMe SSD |
| **Compilación (desde código)** | Rust (versión fijada en `rust-toolchain.toml`) + MSVC C++ Build Tools | Visual Studio Build Tools 2022 |

> **Perfiles automáticos de memoria:**
> - **VRAM (auto-detectada):** ajusta automáticamente `WindowsBarMb`, topes de buffers en tarjeta, `MipDrop` y perfil Series S/X para tarjetas de **8 GB**, **12 GB**, **16 GB** y **24 GB+**.
> - **RAM (`<= 24 GiB`, incluyendo 16 GB, o `< 8 GiB` de commit libre):** `ram_profile.rs` activa automáticamente políticas de retención conservadora y `VICTORY_RESIDENCY=1` sin afectar equipos de 32/64 GB.

---

## Guía Rápida Paso a Paso (Sin Programar)

### Paso 1 — Descargar y compilar una sola vez
Abre una terminal de **PowerShell normal** (no hace falta modo Administrador) dentro de la carpeta del repositorio y ejecuta:

```powershell
cargo build --locked --release -p victory -p victory-launcher
```

Esto genera:
- `target\release\VIctory-RTX.exe` — Launcher gráfico nativo en español (apertura en `< 400 ms`).
- `target\release\victory.exe` — Motor de ejecución y precompilación por línea de comandos.

*(Opcional, solo para perfilado `-GpuTiming` o capturas de diagnóstico):*
```powershell
cargo build --locked --profile diag -p victory --features diagnostics
```

### Paso 2 — Abrir el Launcher Guiado (`VIctory-RTX.exe`)
Haz doble clic en **`Launch-VIctory-RTX.cmd`** (o directamente en `target\release\VIctory-RTX.exe`).

En el primer inicio, el asistente te guiará en **3 pasos simples**:
1. **Carpeta de tu copia local:** Pulsa **Buscar automáticamente** o **Elegir carpeta** y selecciona la carpeta externa donde guardas tus archivos de consola (nunca dentro de la carpeta del repositorio). El launcher genera `victory.toml` automáticamente.
2. **Preparar gráficos (1 clic):** Traduce los shaders a DXIL Wave32 **v6** (`~975 MB`) y precalienta la biblioteca de pipelines (`host-pipelines.bin`, `~255 MB`) dejando 2 núcleos libres para tu PC. Solo necesitas hacerlo una vez (o tras actualizar el traductor) para evitar congelamientos y tirones al jugar.
3. **Elegir perfil y jugar:** Selecciona el perfil acorde a tu tarjeta gráfica y pulsa **JUGAR**.

En **Ajustes → Motor del perfil** puedes guardar el motor junto al nombre del perfil. El perfil local **Nítido 1080p** usa salida 1920×1080. Para repetir su preparación desde PowerShell, ejecuta `Start-Process .\target\release\VIctory-RTX.exe -ArgumentList --prepare-current-profile -Wait -PassThru` y comprueba `ExitCode`.

Para jugar con el perfil guardado desde PowerShell, usa `--play-current-profile` en el mismo comando. Aplica los ajustes del botón **Jugar**, muestra el nombre del perfil coincidente en el registro y espera a que cierre el motor.

Si desaparecen superficies u objetos en el motor local con Series X, abre **Ajustes → Pantalla y Calidad Gráfica** y activa **Recuperar terreno en mallas indirectas (Series X)**. Guarda el ajuste con otro nombre de perfil si quieres comparar la misma escena y vuelve a iniciar el juego. Los perfiles anteriores cargan con la casilla desactivada.

También puedes elegir el ejecutable externo de VIctory-NVIDIA. Con Series X, su modo **1080p → 1440p** renderiza a 1920×1080 y presenta a 2560×1440. **DLSS Calidad + FG 2×** usa la resolución interna de DLSS y requiere las bibliotecas oficiales de NVIDIA Streamline. **F1** guarda ajustes de ese modo. **Jugar** abre el juego y **Precargar y jugar** recorre el catálogo del motor antes del arranque. Cada modo externo tiene su propia caché. Los dos comparten una carpeta de partidas independiente de la del motor local. [Publicaciones de VIctory-NVIDIA](https://github.com/trustno1-coder/VIctory-NVIDIA/releases).

### Paso 3 — ¿Qué perfil gráfico elegir?

| Perfil en el Launcher | Resolución de salida | Escala interna | Ideal para |
|---|---|---|---|
| **Recomendado (`Auto`)** | 720p (8 GB) / 1080p (12–24 GB+) | Automática | Empezar sin complicaciones con valores calibrados por VRAM |
| **Fluido 720p** | `1280×720` | `1.000` nativo (Series X) / dinámica (Series S) | Imagen nítida 720p con máximos FPS y mínima espera GPU |
| **Equilibrado 1080p** | `1920×1080` | `0.667` (`720p` interno → `1080p`) | Mejor balance nitidez/FPS en RTX 3070/4070/4090 |
| **Nítido 1080p** | `1920×1080` | `1.000` (100% nativo) | Series X con espera de texturas, FSR apagado y preparación para Smooth Motion desactivada |
| **Alta 1440p** | `2560×1440` | `0.667` | Monitores 2K con 16–24 GB VRAM |
| **4K FSR 3.1** | `3840×2160` | Según modo FSR | Requiere la DLL oficial firmada de AMD FidelityFX SDK (`amd_fidelityfx_upscaler_dx12.dll`) en `%LOCALAPPDATA%\victory\fidelityfx` |
| **Modo Seguro** | `1280×720` | `0.667` (sin parches agresivos) | Diagnóstico si ocurren cierres inesperados o fallos gráficos |

> **Controles rápidos dentro del juego:**
> - **`F3`**: Activa/desactiva el monitor de rendimiento en vivo (FPS, frametime, draws, VRAM local/compartida y CPU).
> - **Mando:** Compatible con mandos Xbox (XInput) y PlayStation (DualShock 4 / USB).
> - **Teclado y Ratón:** Activable desde el launcher o con `-Kbm` (`F2` o `` ` `` bloquea/desbloquea la cámara; `Esc`/`P` libera el cursor).

---

## Uso por Línea de Comandos (`scripts\play-nvidia.ps1`)

Si prefieres PowerShell en lugar del launcher gráfico:

1. Copia `victory.example.toml` a `victory.toml` y edita `game_dir` apuntando a tu carpeta externa.
2. Precompila shaders y pipelines una sola vez (las variables del preset coinciden automáticamente con `run`):

```powershell
.\target\release\victory.exe shaders --nvidia-preset --jobs 16
.\target\release\victory.exe warm --nvidia-preset --used --jobs 16 --save-library
```

3. Ejecuta con el perfil que prefieras:

```powershell
# Equilibrado 1080p (recomendado, auto-detecta tu nivel de VRAM)
.\scripts\play-nvidia.ps1

# 720p nativo nítido (escala 1.0)
.\scripts\play-nvidia.ps1 -Full720p -Scale 1.0

# 1080p nativo más nítido (escala 1.0)
.\scripts\play-nvidia.ps1 -Scale 1.0

# Con teclado y ratón
.\scripts\play-nvidia.ps1 -Kbm -MouseSens 1.25

# Convolución DSP en CPU para diálogos filtrados (llamadas / aeronaves)
.\scripts\play-nvidia.ps1 -DspConvolution

# Copias paralelas de buffers grandes (>= 8 MiB)
.\scripts\play-nvidia.ps1 -WideCopy

# Con 4K FSR 3.1 (requiere DLL oficial de AMD FidelityFX SDK)
.\scripts\play-nvidia.ps1 -Fsr4K balanced

# Perfilado GPU de 15s (usa build diag con --features diagnostics)
.\scripts\play-nvidia.ps1 -GpuTiming

# Modo seguro sin optimizaciones agresivas
.\scripts\play-nvidia.ps1 -Safe
```

---

## Qué Incluye Este Fork (`nvidia-rtx4090`, Traductor `v6`)

- **Traductor Wave32 `v6` (`victory-shader`, `SHADER_OPT=191`):** aritmética **FP16 nativa** (`SM 6.2`), **48 KiB de memoria compartida (LDS)** en NVIDIA, enlace **Mesh+Pixel** bajo el límite de 32 KiB, tickets de hilos para grupos 2-D (`VICTORY_THREAD_TICKET=1`, `64/64` hilos verificados en RTX 4090 frente a `32/64` heredado), selección de máscaras de carril en Wave32, propagación SCC en `s_min`/`s_max`, asignación de tiles de transmisión (`water_tile_append`), emparejamiento Wave64 por defecto en 28 shaders críticos (incluyendo iluminación de agua abierta reactivada por defecto) y `VICTORY_NAN_SCRUB=1`.
- **Iluminación Ambiental RT-Off Calibrada (`ambient.rs`, `lightprobe.rs`):** acotado geométrico de luminancia y tonalizado hacia el color de la sala en `clear_refl` (elimina el tinte azul/morado en carteles y papeles de interiores junto a ventanas), piso de exposición difusa (`VICTORY_AMBIENT_EXPOSURE_FLOOR=0.0045`) que recupera detalle en escenas a contraluz, limpieza de NaN/Inf y sondas magenta, y huella completa en planos de video lineales `R8_UNORM`.
- **Puertos de Renderizado 1, 2, 5, 6, 7 y RT + CPU Submit:** lectura in-place de CBVs de tabla en heap NVIDIA de 1M (`VICTORY_PORT_CBV=1`), guardas compartidas (`VICTORY_PORT_GUARD=1`), diario de heaps (`VICTORY_PORT_JOURNAL=1`), envío en una sola pasada (`laid_out`) + atajo ordenado (`joined`), pool de upload buffers (`UPLOAD_SPARE`), prioridades de hilos de DirectStorage (`-1`) y helpers (`ABOVE_NORMAL`), listas delgadas paralelas (`VICTORY_THIN=1`) y envío asíncrono (`VICTORY_ASYNC_SUBMIT=1`).
- **Audio, Memoria y Estabilidad:** protección de vistas crudas fuera de límites (`VICTORY_RAW_VIEW_CLAMP=1`), reutilización de páginas físicas de 21 bits en `xmem.rs` para sesiones largas, espera acotada de valla de mapeo (`VICTORY_MAPPING_WAIT_MS`), convolución DSP opcional (`-DspConvolution`) para diálogos filtrados, ventanas ReBAR, `mimalloc`, perfil automático `<= 24 GiB` RAM / `< 8 GiB` commit y limpieza por lotes (`VICTORY_BATCHED_SWEEP=1`).

### Rendimiento Medido de Referencia (RTX 4090 · i9-12900KF · 64 GB RAM · RT Off)

La pasada de 15 minutos a 1080p nativos dio 16,8 FPS de mediana y 8,0 de mínimo en la lancha tras excluir menús y transiciones observados. El launcher 0.9.1 permite reproducir el perfil desde **Jugar → Calidad → Nítido 1080p**. Los tramos históricos de 720p y 1080p reescalados de la tabla corresponden a otras cargas. [Informe de calidad, mediciones y próximas pruebas](docs/rtx4090-informe-2026-11-27.md)

| Escenario (`release`, traductor `v6`) | FPS en el tramo medido | Espera GPU | Notas verificadas |
|---|---|---|---|
| **Menús / Interfaz** | **60.0 FPS** (`16.7 ms`) | `0.0 ms` | Tope de 60 FPS sin alterar velocidad de simulación |
| **Fluido 720p (`1280×720`, `Scale 1.0`)** | **22.6–26.6 FPS** (`24.5 FPS` en ~5,000 draws) | `0.0–1.8 ms` | `prep` bajó a `26.1 ms`, `0` warnings/errors, `0` errores en `372` skin-bake draws |
| **Equilibrado 1080p (`Scale 0.667`)** | **20.1–26.0 FPS** | `0.0–0.6 ms` | **100% cache hit (`3,173 hits, 0 misses`)**, caché limpia de `974.9 MB` + `254.7 MB` pipelines |

---

## Solución de Problemas Frecuentes

| Síntoma | Causa probable y solución |
|---|---|
| **La caché en `%LOCALAPPDATA%\victory\cache\shaders` ocupa varios GB** | Al actualizar entre versiones del traductor se acumulan generaciones viejas. Cierra el juego/launcher, respalda `cache\pipelines\used-pipelines.bin` (si existe), borra `cache\shaders` y `cache\pipelines\host-pipelines.bin` y vuelve a ejecutar **Preparar gráficos** (o `victory shaders --nvidia-preset` + `victory warm --nvidia-preset --used --save-library`). Una generación v6 limpia pesa **`~975 MB`** en shaders + **`~255 MB`** en pipelines. |
| **Interiores demasiado oscuros tras actualizar desde una versión vieja** | El launcher migra automáticamente `launcher.json` (`config_version: 2`) subiendo `ambient_most` a `4.0` y `ambient_reflect` a `1.2`. Si personalizaste valores manualmente, verifica en Ajustes que el techo ambiental sea `4.0` y reflejos `1.2`. |
| **Diálogos silenciosos durante llamadas telefónicas o vuelos** | Activa `-DspConvolution` (o `VICTORY_DSP_CONVOLUTION=1`) para habilitar la convolución DSP en CPU de voces filtradas. |
| **Tirones fuertes o congelamientos largos en tarjetas de 8–12 GB** | Desbordamiento de VRAM hacia memoria compartida PCIe. Usa el perfil **Recomendado** (activa `Series S`, `MipDrop=2` y tope BAR seguro) o baja la calidad de texturas a **Media**. |
| **¿Dónde están los registros (logs)?** | En `%LOCALAPPDATA%\victory\data\nvidia-run-*.log` y directamente desde la pestaña **Logs** del launcher (la exportación redacta usuario, perfil y carpeta local del juego automáticamente). |

---

## Verificación para Contribuidores

Antes de abrir un Pull Request, verifica siempre la política de activos, Clippy y las pruebas:

```powershell
cargo run --locked -p xtask -- policy
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --locked --release -p victory-shader --lib -- divergent_lds_atomics
cargo test --locked --release -p victory-gpu --lib -- torn ambient warp_card_compare host_ram_card_staging changed_key12 room_only_wakes idle_sweep_restarts
cargo test --locked -p victory-launcher --bin VIctory-RTX
```

---

## Créditos y Licencia

Construido sobre el ecosistema open-source **VIctory** (preservando todo el historial Git y autoría original). Consulta la tabla completa de atribuciones en [README.md](README.md#credits--attribution).

Licenciado bajo **GPL-3.0-or-later** — consulta [LICENSE](LICENSE) y [docs/asset-policy.md](docs/asset-policy.md).
