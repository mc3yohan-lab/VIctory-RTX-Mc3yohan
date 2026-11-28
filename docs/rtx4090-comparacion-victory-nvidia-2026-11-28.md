# VIctory-NVIDIA y nuestra RTX 4090 · 28 de noviembre de 2026

La referencia de esta comparación es la versión publicada `v2026.11.28.01` (`63b902f`). La base local es `nvidia-rtx4090` en `e322d0a`. El criterio para elegir motor sigue siendo calidad gráfica en la oficina, la piscina y la lancha, seguida de 30 FPS renderizados durante 15 minutos de control con un mínimo de 20 FPS por ventana. [Prueba local anterior](rtx4090-informe-2026-11-27.md)

## Qué cambió en el fork

| Versión | Cambio relevante para la comparación |
|---|---|
| .8 | El autor recuperó la iluminación del agua y corrigió bloques oscuros, defectos de nubes y dominante violeta. También registró un coste de FPS. [Notas de .8](https://github.com/trustno1-coder/VIctory-NVIDIA/releases/tag/v2026.11.27.8) |
| .10 | Evita inicializar la generación de cuadros en las sesiones que arrancan con ella apagada, recupera la traducción de un shader submarino y permite limitar la concurrencia de la precarga. El shader submarino ya llegó a crear un pipeline en el registro de nuestra 4090. La calidad visual bajo el agua queda por comparar. [Notas de .10](https://github.com/trustno1-coder/VIctory-NVIDIA/releases/tag/v2026.11.27.10) |
| .28.01 | F1 permite elegir 1080, 720, 626 o 540 líneas internas para Series S. La salida sigue a 1080p y cambiar el tamaño requiere reiniciar. Con DLSS apagado el juego usa 720 líneas internas. La versión también reactiva la devolución de ranuras de texturas, el recambio de asignadores de comandos y la agrupación de pequeños búferes. [Notas de .28.01](https://github.com/trustno1-coder/VIctory-NVIDIA/releases/tag/v2026.11.28.01) |

El launcher local ya traduce shaders y precalienta los pipelines que registró la partida. En F1 interesa poder distinguir qué ajustes tienen efecto en el cuadro siguiente y cuáles requieren reiniciar. La opción de Series X `1080to1440` usa 1080p internos con salida a 1440p. Una prueba entre motores debe registrar ambas dimensiones, la consola emulada, el estado de la caché y los FPS renderizados. [Opciones del fork](https://github.com/trustno1-coder/VIctory-NVIDIA/blob/63b902fe1720a5188ee79d94a88c8de0470f6349/apps/victory/src/play.rs) · [Preparación local](../apps/victory-launcher/src/runner.rs)

## Cambios adaptados

El launcher integra ambos motores. Cada perfil guarda su elección y respeta la configuración y las cachés del motor correspondiente. Para comparar el rendimiento entre ellos también deben coincidir la escena y las resoluciones interna y de salida.

La comparación incluyó los ajustes para 8 GB de Yaekai, la gestión de versiones y perfiles de VILauncher, la reconstrucción de pipelines de Kemerd y las correcciones de texturas y descriptores de OP-lite. En la prueba de la 4090 se conservaron el nivel completo de los mipmaps y los pases de iluminación del agua. Se incorporaron perfiles independientes y una corrección de descriptores que se pudo reproducir localmente. [Comparación por repositorio y enlaces al código](rtx4090-informe-2026-11-27.md)

**Precarga de pipelines.** Con un libro sintético que referenciaba un shader ausente, el motor anterior indicó que había preparado un pipeline aunque el proceso comunicó que carecía de un pipeline de host. La segunda ejecución lo omitió. Se adaptó la corrección de Yorivel y se usa un registro de aciertos distinto para volver a verificar las entradas ambiguas de instalaciones anteriores. Se conservan la biblioteca del controlador y los fallos de compilador que ya estaban identificados. [Cambio de Yorivel](https://github.com/trustno1-coder/VIctory-NVIDIA/commit/5ed66db74f6d5c634a105f65f7803e61455f2db9) · [Prueba de la CLI](../apps/victory/tests/warm.rs)

**Texturas parcialmente residentes.** Al adelantar la vista de una textura hasta su primer mip residente, el límite de detalle sigue medido desde el mip cero de ese mismo recurso. En D3D12 WARP, con límite 3, las vistas ajustada y original devolvieron 48 y 64 antes del cambio, y 64 y 64 después. Con límite fraccionario 3,5 ambas devolvieron 72. El perfil de la partida con personajes poligonales tenía desactivada esta ruta opcional. La explicación de esa escena sigue abierta. Cambio adaptado del trabajo de BradRoland y fxceless. [Propuesta original](https://github.com/fxceless/victory-fork/pull/1210) · [Integración en VIctory-NVIDIA](https://github.com/trustno1-coder/VIctory-NVIDIA/commit/d8b258a455e50035369e934b597131c00881bceb) · [Implementación y prueba local](../crates/victory-gpu/src/tiles.rs)

## Observación en la RTX 4090

El SHA-256 del ejecutable oficial coincidió con el publicado. La prueba usó Series X con escena y salida a 2560×1440, DLSS y generación de cuadros desactivados, y copias privadas de los datos de partida. Se observó el menú F1 renderizado sobre el juego mediante su opción de apertura al inicio. El menú identificaba los cambios en vivo y los que requerían reinicio. La pulsación física de F1 quedó sin verificar.

Con caché fría, la captura de la ventana pasó de una ilustración de carga en color a una imagen casi blanca y luego recuperó la oficina. En muestras exteriores se observaron una cabeza ausente y un gorro desconectado del cuerpo. En otro arranque con caché procedente de una pasada anterior se reconocieron la patrulla y la familia antes de la oficina. El estado de la caché incluía imágenes, shaders y pipelines. La comparación no aísla el efecto del comando de precarga.

**Piscina.** La imagen de la versión oficial con caché reutilizada y listas rápidas activas corresponde al mismo plano de los dos bañistas con sombrero que en nuestra prueba tenía brazos facetados y superficies planas. Se distinguen la forma del ala del sombrero, el pelo, la piel, la tela, el relieve de las tumbonas y personajes de fondo. El contador de la imagen marcó **4,8 FPS**. En la oficina se observaron ventanas de 29–34 FPS en la pasada fría y en exteriores caídas a 3–5 FPS. Son observaciones de cinemáticas con carga de validación concurrente, a distinta resolución que los 1080p de la prueba anterior. La estabilidad durante 15 minutos, el rendimiento a igualdad de resolución y la comparación con Series S quedan abiertos.

**Listas rápidas.** Las dos pasadas de aproximadamente 218 segundos partieron de copias con huellas idénticas de 2.219 archivos de caché y diez archivos de perfil y partida. Se cambió únicamente el ajuste público `THIN`. Con el valor 1, la piel de la mujer con gafas se vio natural en dos planos. Con 0, en ambos apareció un mosaico magenta y verde, y en la piscina se vio una banda rosa sobre la espalda de la bañista. La geometría detallada de ambos bañistas apareció con los dos valores. Estos planos proceden de un arranque por variante. En esa configuración del motor oficial, desactivar `THIN` también deja sin uso la cola que agrupa los envíos, aunque el valor de `JOIN_PARTS` permanece en 8. El perfil de nuestro motor activa explícitamente los envíos asíncronos. [Experimento publicado en la RTX 5080](https://github.com/trustno1-coder/VIctory-NVIDIA/commit/5986a64dd766702cc3dea146ad29bf5e4331c196) · [Condición de la cola del fork](https://github.com/trustno1-coder/VIctory-NVIDIA/blob/v2026.11.28.01/crates/victory-gpu/src/queue/behind.rs#L748-L764)

**Resolución interna.** Con `THIN=1`, el mismo checkpoint y la misma caché de partida inicial, se probó `--res 1080to1440 --dlss off`. El motor confirmó escena 1920×1080 y salida 2560×1440. En el plano de los dos sombreros el contador marcó **15,9 FPS**. Se conservaron el pelo, la forma de los cuerpos y la tela. La fachada de fondo mostró menos detalle. El modo creó su propia imagen de resolución en caché y parte de la validación local se solapó con los recorridos. Los **4,8 y 15,9 FPS** son lecturas puntuales del mismo plano, insuficientes para estimar cuánto gana el recorrido completo o identificar por sí solas el cuello de botella.

El siguiente recuento incluye sólo muestras del título de la ventana con al menos 1.000 dibujos por cuadro. Hubo lecturas repetidas y la ruta era cinematográfica. Cada pasada duró unos 220 segundos.

| Escena → salida | Muestras | FPS mínimos | FPS de mediana | FPS máximos | Muestras bajo 20 |
|---|---:|---:|---:|---:|---:|
| 1440p → 1440p | 107 | 1 | 23 | 35 | 38 |
| 1080p → 1440p | 97 | 6 | 25 | 36 | 22 |

La ventana del fork desapareció al cerrarla, pero su proceso siguió activo y hubo que terminarlo después de verificar su identidad. Los 14 archivos personales de referencia y el perfil del launcher conservaron sus huellas. El cierre del proceso también debe resolverse antes de intercambiar motores desde el launcher.

**Elección del motor.** Se conserva el motor local como base a 1080p nativo. La versión oficial con listas rápidas activas sirve como referencia de materiales y ofrece DLSS y F1 desde un segundo perfil. Para adoptar otro motor por rendimiento hace falta una comparación repetida con las mismas dimensiones, ruta y condiciones de caché.

## Verificación de la rama local

En Windows pasaron `cargo check --workspace`, Clippy para todos los targets y todas las features con `-D warnings`, y la suite completa con un hilo de prueba (**1.220 aprobadas**). Las cuatro pruebas dirigidas de texturas incluyeron el muestreo gráfico D3D12 WARP que la suite habitual omite. Compilaron en release el motor y el launcher. Las comprobaciones usaron las dependencias bloqueadas y el modo offline. La auditoría de assets pasó. En el ejecutable release, una segunda precarga reintentó correctamente el pipeline sintético rechazado incluso con un registro antiguo, y omitió uno ya marcado como listo en el nuevo formato. La mejora de tiempos de juego de estas dos correcciones queda por medir.

## Perfiles del launcher y precarga en 1080p

El launcher guarda el motor como parte de cada perfil. El perfil local de calidad usa Series X, escala 1, espera de texturas y salida 1920×1080. En el motor VIctory-NVIDIA, el modo Series X sin DLSS usa escena 1920×1080 y salida 2560×1440. Con DLSS Calidad y generación 2×, el mismo motor midió **1706×960 → 2560×1440**. Las cachés de esos modos están separadas. El menú F1 conserva sus ajustes por modo y el launcher sigue el proceso hijo si el motor se reinicia. [Release externa y SHA-256](https://github.com/trustno1-coder/VIctory-NVIDIA/releases/tag/v2026.11.28.01)

En la prueba de presentación de Streamline 2.14.16 con la RTX 4090, 180 presentaciones de entrada produjeron 360 cuadros mostrados. En el juego se verificaron los vectores de movimiento, la profundidad y el primer present de generación con estado 0x0. Tras el reinicio de calentamiento, en el exterior se veían el personaje, los árboles y una columna de humo. Parte del primer plano seguía negra, con halos de color en algunos bordes. En esa muestra el panel marcó **3,5 FPS renderizados, 7 mostrados y 285,5 ms por cuadro**. Había precargas concurrentes. Esta medición comprueba la activación de FG y permite evaluar la imagen del fotograma. La meta de estabilidad se evalúa en una pasada de juego sin generación.

En el motor local se corrigió el registro de precarga para reintentar los pipelines que el driver no pudo crear. En la traducción real de este perfil había 25.784 shaders en caché y 201 incompatibles. Se comprobaron 94 archivos de entrada y se omitieron 16 por el límite de tamaño de lectura del traductor. Un pipeline que pasó 180 segundos sin responder quedó en cuarentena en la primera tanda de compilación.

## Tiempo de bloqueo al actualizar texturas

Se adaptó la propuesta de alexis27yt que acorta el bloqueo de la memoria del título al refrescar texturas. La ruta opcional `VICTORY_TEXTURE_SHORT_HOLD=1` suelta el bloqueo tras copiar los bytes necesarios. En la ruta directa identifica algunos cambios con ocho bytes de muestra. Cuando la muestra coincide, verifica el contenido completo. El camino predeterminado continúa disponible para comparar el coste. [Propuesta original #1078, bajo GPL](https://github.com/fxceless/victory-fork/pull/1078)

En una prueba de CPU en el i9-12900KF con nueve pares alternados y la precarga en curso, la espera mediana del escritor al cambiar un byte muestreado bajó de 0,506 a 0,307 ms en BC7 de 2048² y de 1,982 a 1,065 ms a 4096². El trabajo total del lector subió respectivamente de 0,493 a 0,830 y de 1,970 a 3,055 ms. La salida coincidió byte por byte. Se mantiene desactivada de forma predeterminada hasta medir su efecto en cuadros de juego.


## Comprobación del suelo y de las constantes de dibujo

El suelo del exterior también apareció oscuro en una referencia de Series X sin DLSS, con una cámara distinta. En el modo DLSS Calidad + FG 2× se repitió la escena con la copia de retorno de GPU a CPU habilitada. Las chimeneas, los árboles y el vehículo permitieron reconocer la localización en dos capturas. La superficie permaneció lisa y oscura. No se obtuvieron tres pares de imágenes con encuadre idéntico. Se conservó el ajuste de partida. La combinación alternativa Series S + DLAA y FG 2× alcanzó la inicialización de Streamline, pero se cerró durante la carga antes de la primera escena. El selector ofrece los dos modos de Series X comprobados en juego.

Se reprodujo un defecto independiente de los colores del suelo. Cuando una espera divide una lista de comandos, un descriptor de constantes puede seguir apuntando a una copia hecha para el segmento anterior. Dos dibujos con los mismos identificadores de tabla y valores sucesivos A y B leyeron A/A en el renderizador D3D12 de software de Microsoft (WARP). Al restaurar el estado se invalidan únicamente las tablas que necesitaron copiar esas constantes. El segundo segmento obtiene su propia copia y la GPU lee A/B. La tabla que solo contenía una vista de lectura conservó su ranura. La prueba usa la implementación de tablas y restauración de la propia rama. La relación de este defecto con una malla individual del juego requiere verificación en esa escena. [OP-lite](https://github.com/FilipGra24/OP-lite)

## Precarga de las escenas jugadas

La primera preparación a 1080p encontró 83.559 pipelines conocidos en el motor local. De los 3.049 registrados durante partidas, el driver creó 3.046 en procesos aislados, dos dependían de shaders de cómputo Wave64 sin traducción y un trabajador agotó el límite de 180 s. La fase aislada duró 1.329 s. El segundo paso tardó 565,7 s y confirmó los mismos 3.046. La biblioteca conservó su tamaño y fecha previos (274.594.010 bytes). En la RTX 4090, los pipelines con samplers estáticos se recrean por la ruta ya comprobada de este proyecto. El perfil de texturas utilizó `PORT_CONSTANTS=1`, `PORT_CONSTANTS_ORDER_ALONE=0` y espera de texturas, como en el A/B documentado. [Auditoría previa de samplers y constantes](rtx4090-informe-2026-11-27.md)

En VIctory-NVIDIA se tradujeron o recuperaron de la caché 25.760 shaders y quedaron 225 sin traducción. Se priorizaron los pipelines usados por este recorrido. Con el mismo perfil, la precarga selectiva procesó los 776 que faltaban en 138 s, con cero rechazos y cero trabajadores agotados. Su registro terminó en 2.073 de 2.073. El catálogo completo de 83.569 incluye pipelines que estas partidas no solicitaron. El registro de intentos del fork por sí solo no acredita que el driver aceptó cada entrada histórica. Cada motor utilizó su propia caché.

El pipeline que agotó el tiempo pasó por separado en 51,3 s. Se repitió la preparación desde el launcher y el controlador llegó a crear 3.047 de los 3.049 usados. Los dos pendientes se mantuvieron fuera de la lista de aciertos.

## Geometría de la piscina a 1080p nativo

La propuesta comunitaria de invismann identificó una causa de suelo ausente al dibujar mallas con comandos indirectos. El cambio local conserva el número de grupos del eje X y usa una sola fila y una sola capa para esta ruta de Series X. El despacho de cómputo tridimensional sigue usando sus tres dimensiones. Se probaron tanto la traducción directa como el procesamiento por lotes con D3D12 WARP. El ajuste puede guardarse en un perfil local desde **Ajustes → Pantalla y Calidad Gráfica → Recuperar terreno en mallas indirectas (Series X)**. [Propuesta original #996 de VIctory-NVIDIA](https://github.com/trustno1-coder/VIctory-NVIDIA/pull/996)

Se repitió el mismo guardado privado en la RTX 4090 con escena y salida a 1920×1080, escala 1 y generación de cuadros apagada. En el plano de las dos personas con sombrero se alinearon dos luces y ambos sombreros. Aparecieron las tumbonas, los parasoles y el suelo iluminado del recinto. En dos zonas de suelo de 80×40 píxeles se contaron los píxeles con los tres canales por debajo de 16. En cada zona la cuenta pasó de 3.200 a cero. La luminancia media subió de 9,84 a 215,36 en una y de 6,68 a 220,71 en la otra, en una escala de 0 a 255.

Los personajes conservaron contornos angulosos. La oficina mostró un resultado similar a su referencia anterior. En esta prueba faltó un encuadre equivalente de la carretera de Key Lento para acreditar la misma recuperación allí. La caché creció durante la primera pasada, por lo que esta comparación mide la diferencia de imagen. La cadencia se midió por separado en juego.

## Qué cuesta un cuadro en la lancha

En los cinco primeros tramos de navegación con HUD y movimiento se aislaron 26 ventanas completas. La mediana fue de 16,7 FPS, con mínimo de 5,9. En esas ventanas el cuadro costó 59,9 ms de mediana y la preparación 33,35 ms, con 9,65 ms para copias y 6,9 ms para texturas. Las copias de retorno en paralelo costaron 15,5 ms de mediana. Los contadores se solapan. En 35 lecturas del sensor la GPU mostró una mediana del 95 % de uso y 17.985 MiB de memoria ocupada en el equipo.

Al ampliar la lectura a ocho tramos, cinco muestras jugables por debajo de 10 FPS y once a 20 FPS o más permitieron comparar contadores del mismo segundo. En las lentas, la espera de mapeo al actualizar texturas fue de 65,1 ms de mediana y la GPU se quedó sin trabajo en cola durante 84,2 ms. En las rápidas fueron 7,4 y 1,5 ms, respectivamente. La copia de retorno cambió mucho menos, de 15,2 a 16,3 ms. La asociación señala dónde probar un cambio que conserve la resolución y los mipmaps.

El registro también informó de 688 comandos omitidos por cuadro de mediana y de una cola global de unos 670 pipelines. Son magnitudes distintas. Un mismo pipeline puede servir para muchos dibujos y la cola incluye trabajos anticipados. La sesión leyó una lista de 3.724 entradas para arranque anticipado y registró al menos 1.024 recreaciones con samplers estáticos. Dos shaders de cómputo Wave64 quedaron sin traducir. Con el detalle disponible no se puede asignar los dibujos omitidos a esos dos shaders.

La versión de diagnóstico de una pasada anterior de la lancha sí separó los motivos. En tres ventanas figuraron tanto constructores de trazado de rayos desactivados como shaders omitidos por el perfil. Para identificar el siguiente defecto visible hay que comparar el primer material perdido con su motivo de descarte en el mismo instante, conservando la cámara y el ajuste de terreno. Esa medición se puede hacer en un tramo corto, con los temporizadores de GPU desactivados para evitar alterar la prueba principal. [Contador de comandos](../crates/victory-gpu/src/draw.rs) · [Cola de pipelines](../crates/victory-gpu/src/pipeline/make.rs)

## Partida hasta el campamento

La pasada de 25 minutos alcanzó Sunrise RV Park y el objetivo de rescatar a Donnie. Se verificaron 8 min 35 s de juego efectivo. El tramo continuo más largo duró 1 min 56 s. La navegación requirió reintentos y se usó una vez la opción del juego para omitir un punto de control. Se excluyeron cinemáticas, fallos, menús y momentos de control incierto. Los 14 archivos de usuario comprobados y la configuración del launcher conservaron sus huellas al cerrar.

| Medida en las secciones jugables | Resultado con el perfil de terreno |
|---|---:|
| Ventanas de rendimiento completas | 84 |
| FPS nativos mínimo / mediana / máximo | 5,3 / 15,5 / 30,3 |
| Ventanas por debajo de 20 FPS | 73 de 84 |
| Ventanas a 30 FPS o más | 1 de 84 |
| Intervalos entre presentaciones DXGI | 7.652 |
| Percentil 95 / 99 entre presentaciones | 127,0 / 250,2 ms |
| Intervalos de más de 50 ms | 5.419 (70,8 %) |

En 37 muestras del campamento la cola de pipelines pendientes ya estaba vacía. Aun así se omitieron entre 913 y 1.176 comandos por cuadro, con mediana de 1.080. Por eso la precarga de estas escenas y la investigación de los materiales ausentes se miden por separado. La biblioteca que dejó la partida ocupó 278.831.000 bytes. Entre los pipelines registrados y reproducibles por la herramienta, la partida amplió el conjunto de precarga a 3.252.

## Verificación del launcher publicado

La rama con el ajuste de terreno por perfil pasó Clippy y ambas suites de GitHub Actions para Windows. Pasaron 1.227 pruebas en configuración normal y 1.281 con diagnóstico. Las pruebas incluyen perfiles anteriores, nombres personalizados y que la casilla afecte únicamente al motor local Series X. Se compiló el launcher en release y se conservó la misma compilación del motor para las comparaciones. [CI de la revisión 43b24d1](https://github.com/mc3yohan-lab/VIctory-RTX-Mc3yohan/actions/runs/46146751601)
