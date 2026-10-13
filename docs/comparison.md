# RustGIS vs. other GIS platforms

How RustGIS compares to its sibling project **[GeoLibre](https://geolibre.app)** and to the GIS
platforms people most often ask about: **QGIS**, **ArcGIS Pro** and **ArcGIS Online**.

RustGIS is not trying to replace any of these. It fills a narrower spot: a **free and
open-source desktop GIS compiled to a single native program**, with a ribbon interface familiar
from desktop GIS, every action available as a scriptable command, and the same app running in a
browser tab through WebAssembly.

!!! note "How to read this"
    Comparisons like this age quickly and are written by an interested party. The RustGIS
    column reflects what ships in the [current source](https://github.com/opengeos/RustGIS)
    as of **October 2026**; the other columns describe each product's mainstream,
    out-of-the-box behaviour, not what is reachable with every add-on or plugin. The GeoLibre,
    QGIS and ArcGIS columns draw on [GeoLibre's own comparison](https://geolibre.app/comparison/),
    where those products are described in more depth. Found something wrong or out of date?
    Please [open an issue](https://github.com/opengeos/RustGIS/issues).

## RustGIS and GeoLibre

RustGIS and GeoLibre are both [opengeos](https://github.com/opengeos) projects, and they share
more than a maintainer: both run the same suite of over 1,000 GeoLibre and Whitebox
geoprocessing tools, natively on the desktop and on WebAssembly in the browser.

They differ in how they are built and who they suit:

- **GeoLibre** is built on web technology (Tauri, React, MapLibre GL JS, DuckDB-WASM, deck.gl).
  It reaches further: cloud-native formats (GeoParquet, PMTiles, Zarr, STAC), databases, OGC
  and Esri services, native Android and iOS apps, a Jupyter widget, a Python package and
  real-time collaboration.
- **RustGIS** is written entirely in Rust and draws its own interface on the GPU (egui). It is
  one native program with no browser engine or JavaScript runtime inside, a ribbon laid
  out in the order of the work, and an engine where every button is a command with an id, run the
  same way by the ribbon, `rustgis-cli`, notebooks, models and the AI Assistant.

If you want the widest data access and the most platforms, choose GeoLibre. If you want a
compact native desktop GIS with a classic ribbon workflow and a fully scriptable command
engine, choose RustGIS. Both keep your data on your machine.

## At a glance

| | **RustGIS** | **GeoLibre** | **QGIS** | **ArcGIS Pro** | **ArcGIS Online** |
|---|---|---|---|---|---|
| **License** | Apache-2.0, open source | MIT, open source | GPL-2.0+, open source | Proprietary | Proprietary |
| **Cost** | Free | Free | Free | Paid subscription | Paid subscription (credit-metered analysis) |
| **Built with** | Rust (egui, wgpu, tiny-skia) | TypeScript and Rust (Tauri, React, MapLibre) | C++ and Python (Qt) | .NET and C++ | Web (JavaScript) |
| **Desktop** | Windows, macOS, Linux: one native program | Windows, macOS, Linux (Tauri) | Windows, macOS, Linux | Windows only | — |
| **Browser** | Full app compiled to WebAssembly | Full app, nothing to install | No official browser build | No | Yes (the product *is* the browser app) |
| **Mobile** | — | Native Android and iOS apps | Via QField / Mergin Maps (separate apps) | — | Via ArcGIS Field Maps (separate app) |
| **Interface** | Ribbon, tabs in the order of the work | Panels and toolbars | Toolbars and panels | Ribbon with contextual tab groups | Web map viewer and apps |
| **Works offline** | Yes: the desktop app needs no network; offline areas with sync | Yes: PWA install, offline areas, desktop build | Yes | Yes | Limited (Field Maps offline areas) |
| **Where your data lives** | Your device | Your device, processed client-side | Your device | Your device / your enterprise geodatabase | Vendor cloud |
| **Project file** | `.rgis` (JSON, open) | `.geolibre` / `.geolibre.json` (open, documented) | `.qgs` / `.qgz` (open) | `.aprx` (proprietary) | Web map JSON (hosted) |

## Data and formats

| | **RustGIS** | **GeoLibre** | **QGIS** | **ArcGIS Pro** | **ArcGIS Online** |
|---|---|---|---|---|---|
| **Format breadth** | Focused: GeoJSON, CSV, Shapefile, GeoPackage, GeoParquet, FlatGeobuf, KML/KMZ and GPX in and out (large files read in place); GeoTIFF/COG and ASCII grids; WKT out; with GDAL installed, the vector formats it reads; with DuckDB, SQL queries over files and web addresses | Wide: DuckDB-WASM Spatial plus in-house readers | **Widest**: everything GDAL/OGR reads | Very wide, plus native Esri formats | Common upload formats |
| **Cloud-native data** | GeoParquet, FlatGeobuf (large files read in place), PMTiles over HTTP byte ranges, cloud-optimised GeoTIFF (as files) | GeoParquet, FlatGeobuf, PMTiles, COG, Zarr, streamed over HTTP | GeoParquet, FlatGeobuf, PMTiles, COG, Zarr | GeoParquet, COG, Zarr | Hosted layers |
| **Web maps and tiles** | XYZ/TMS/WMS raster tiles, Mapbox Vector Tiles, MapLibre styles | XYZ, WMS/WFS/WMTS, OGC API, vector tiles, MapLibre styles | Full OGC support, vector tiles | Full OGC support | Yes |
| **Coordinate systems** | Rasters in any EPSG code, reprojected on the fly; the map in presets or projected EPSG codes | Reprojects on import, EPSG catalog | Full PROJ support | Full support | Web Mercator focus |
| **Geocoding** | Find Place and address tables through Nominatim or Photon (OSM's public servers or your own) | Geocoding search and batch geocoding | Through plugins | ArcGIS World Geocoder (credits) | Yes |
| **STAC, databases, Esri services** | — | STAC browser, PostGIS, DuckDB, Iceberg, ArcGIS services, File Geodatabase (desktop) | STAC, PostGIS, SpatiaLite, Oracle, SQL Server, ArcGIS services | Enterprise geodatabases, Esri services | Hosted only |
| **3D** | A 3D globe; 2.5D extrusion with lighting and shadows; glTF scene export | Cesium globe, 3D Tiles, I3S, LiDAR, Gaussian splats | Point clouds, 3D map view | **Deepest 3D**: scenes, I3S, mesh, LAS | Scene Viewer |

## Analysis and processing

| | **RustGIS** | **GeoLibre** | **QGIS** | **ArcGIS Pro** | **ArcGIS Online** |
|---|---|---|---|---|---|
| **Geoprocessing tools** | **1,000+** (GeoLibre and Whitebox) plus built-in vector tools | **1,000+** (GeoLibre and Whitebox) | **1,000+** (native, GDAL, GRASS, SAGA) | **Most mature and complete**, plus paid extensions | A useful subset, credit-metered |
| **Where analysis runs** | Your machine; in the browser on WebAssembly | Your browser (WASM); optional Python sidecar on desktop | Your machine | Your machine, or a server | Vendor cloud |
| **Vector analysis** | Buffer, clip, erase, dissolve, summarize within, statistics, selection by location; GeoLibre vector tools | Buffer, overlay, dissolve, joins, topology checks | Comprehensive | Comprehensive | Common tools |
| **Raster analysis** | Raster functions, band math, indices, change detection, classification, viewshed; GeoLibre terrain and hydrology tools | Hillshade, slope, zonal stats, raster calculator, reclassify | Comprehensive (GDAL/GRASS/SAGA) | Comprehensive; Spatial Analyst | Limited |
| **Query language** | SQL `WHERE` clauses for selection and definition queries | DuckDB Spatial, PGlite/PostGIS, Sedona, all in the browser | Virtual layers, PostGIS | SQL against geodatabases | — |
| **Model / batch chaining** | ModelBuilder (models from history), tasks, scripts | Visual Model Builder, batch runner | Graphical Model Designer | ModelBuilder | — |
| **Guided workflows** | Suitability, visibility, line of sight, IDW, shortest path, neighborhood, simulation | Isochrones, routing, Emerging Hot Spot Analysis | Plugins and processing providers | Extensions (Network, Spatial, Geostatistical Analyst) | Routing services (credits) |
| **AI / ML** | Pixel classification (k-means, minimum distance, a trainable neural network) | AI Segmentation (SAM), ONNX object detection | Via plugins | Deep-learning toolset | Pretrained models (credits) |

## Cartography, layout and output

| | **RustGIS** | **GeoLibre** | **QGIS** | **ArcGIS Pro** | **ArcGIS Online** |
|---|---|---|---|---|---|
| **Renderers** | Single, unique values, graduated, proportional; hexagon/square aggregation; raster stretch, RGB, classify, colormap, shaded relief | Single, categorized, graduated, rule-based, heatmap, cluster, proportional | **Richest** | Very rich | Smart-mapping styles |
| **Labeling** | Collision-free placement, scale ranges, annotation | Data-defined engine | Best-in-class placement | Maplex | Basic |
| **Print layout** | Map frames, legends, scale bars, grids, extent indicators, map series; PDF and PNG | Print Layout with atlas, PNG, PDF, SVG | Full composer with atlas | Full layout view with map series | Basic print |
| **Reports** | Grouped reports with statistics, charts and subreports; PDF | — | Via atlas and plugins | Reports | — |
| **Presentations** | HTML slideshow and PDF from map, text, image and video pages | Story maps with presenter view | Via plugins | — | ArcGIS StoryMaps (separate product) |
| **Standalone export** | Web map page (one HTML file), OGC API Features collections, MapLibre styles, glTF | Whole project to one offline HTML file | Via qgis2web | — | — |

## Automation and extensibility

| | **RustGIS** | **GeoLibre** | **QGIS** | **ArcGIS Pro** | **ArcGIS Online** |
|---|---|---|---|---|---|
| **Scripting** | Every action is a command: `rustgis-cli` runs any of them headlessly; a Script pane | Python console, `geolibre` Python package | PyQGIS, `qgis_process` | `arcpy` | ArcGIS API for Python / JavaScript |
| **Notebooks** | Built-in notebooks of text and command cells; Jupyter `.ipynb` in and out | The whole app as a Jupyter widget | Bindings, not the UI | Notebooks drive `arcpy` | Hosted notebooks |
| **Plugins** | Sandboxed WebAssembly plugins with declared permissions; a registry; the same file runs on desktop, CLI and web | TypeScript plugin API, a registry | **Largest**: thousands of Python plugins | .NET add-ins, Python toolboxes | App templates |
| **AI assistant** | Acts through undoable commands; Anthropic, OpenAI, Gemini, OpenRouter, Ollama or any OpenAI-compatible model; bring your own key; also on the command line | Natural-language assistant with undoable operations; bring your own key | Via plugins | Copilot features | AI assistants |
| **Collaboration** | — | Real-time (MVP), review comments | No | No | Shared hosted layers |
| **Self-hosting** | Serve the static web build anywhere | Docker image or static build | QGIS Server | ArcGIS Enterprise (paid) | — |

## Where each one is the right choice

**Choose RustGIS when** you want a compact, native desktop GIS that installs as one program, a
ribbon interface without a licence, every action scriptable from
the command line with the same ids the interface uses, plugins that run sandboxed, or an AI
assistant whose every step is an undoable command.

**Choose GeoLibre when** you need the widest data access from a browser tab (cloud-native
formats, STAC, databases, OGC and Esri services), the same app on Android, iOS and in Jupyter,
real-time collaboration, or a Python package.

**Choose QGIS when** you need the widest format support through GDAL/OGR, the deepest open-source
cartography, a mature plugin for a niche task, or heavy local processing of large datasets.

**Choose ArcGIS Pro when** you are in the Esri ecosystem, need enterprise geodatabases and
versioned editing, the deepest 3D and imagery workflows, or organizational support.

**Choose ArcGIS Online when** the priority is hosted layers, organizational sharing and the
surrounding Esri app family.

They also compose: RustGIS reads and writes GeoJSON, CSV, GeoPackage, GeoParquet, Shapefile and GeoTIFF, which every tool above
opens, and writes MapLibre styles and OGC API Features collections for the web.

## Where RustGIS is not the strongest option

Stated plainly, so the tables above are worth trusting:

- **Format breadth.** There is no database connection (beyond DuckDB files) yet, and the web
  app doesn't write GeoPackages. Other vector formats (GML,
  File Geodatabase, DXF…) need GDAL installed, and SQL queries need DuckDB, which RustGIS calls
  rather than links; GDAL rasters aren't read.
- **Coordinate systems.** Vector data is stored in longitude/latitude; there is no full PROJ.
- **Editing depth.** No vertex editing of existing features yet, and no versioned or
  multi-user editing.
- **Specialized analysis.** Spatial joins, overlay union and intersect, network datasets and
  spatial statistics are thin or missing, beyond what the GeoLibre and Whitebox tools cover.
- **Python.** There is no Python API; automation goes through commands, the CLI and plugins.
- **Maturity.** RustGIS is young. Many features are first versions; see the
  [roadmap](https://github.com/opengeos/RustGIS/blob/main/ROADMAP.md).

## See also

- [User Guide](user-guide/index.md): what RustGIS does, page by page
- [Command Line](cli.md): scripting RustGIS
- [GeoLibre's comparison](https://geolibre.app/comparison/): GeoLibre against QGIS, ArcGIS,
  CARTO, Felt, kepler.gl and Google Earth
