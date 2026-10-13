# Data

## Formats

Every format, with how to add it and its limits, is listed on [Supported Data
Formats](data-formats.md).

| Format | Read | Write |
| --- | --- | --- |
| GeoJSON (`.geojson`, `.json`) | Yes | Yes |
| Delimited text (`.csv`, `.tsv`, `.txt`) with x/y columns in any coordinate system, or WKT geometry | Yes | Yes |
| Shapefile (`.shp` + `.dbf`, or zipped in a `.zip`) | Yes (the web app opens zips) | Yes (`share.exportLayer`) |
| GeoPackage (`.gpkg`, every feature table) | Yes | Yes (desktop) |
| GeoParquet (`.parquet`, `.geoparquet`, WKB geometry) | Yes (zstd-compressed files on the desktop only) | Yes |
| FlatGeobuf (`.fgb`) | Yes | Yes (`share.exportLayer`) |
| KML/KMZ (with their styles), GPX | Yes | Yes |
| Through GDAL: GML, DXF, File Geodatabase (`.gdb` folder), MapInfo, OpenStreetMap… | Yes (desktop, when GDAL is installed) | No |
| DuckDB databases (`.duckdb`, `.ddb`) and SQL queries | Yes (desktop, when DuckDB is installed) | No |
| KML, GPX, CSV with WKT geometry | No | Yes (Analyze › Workflows › Data Interop) |
| GeoTIFF and cloud-optimised GeoTIFF (`.tif`, `.tiff`) | Yes | Yes |
| ASCII grid (`.asc`, with an optional `.prj`) | Yes | No |
| Web tiles: XYZ/TMS/WMS rasters, Mapbox Vector Tiles, MapLibre styles, PMTiles and MBTiles archives | Yes | Yes (Analyze › Create Hosted Imagery) |
| RustGIS project (`.rgis`; older `.rgisx` files open too) | Yes | Yes |

Add data with **Home › Add Data** (**From URL…** downloads a file by its web address; see
[Files on the web](data-formats.md#files-on-the-web)), by dropping files on the window, or by
passing them on the command line (`rustgis places.geojson`). **Home › Add Data › Delimited
Text…** picks a CSV's x and y (or WKT) columns, their coordinate system (such as `EPSG:2263`) and
the separator; Add Data finds them itself (columns named like longitude/latitude,
easting/northing or geometry, and EWKT's `SRID=`) and says when coordinates aren't
longitude/latitude. A CSV without coordinate columns, or a layer without geometry, opens as a
standalone table, listed under the map in Layers. **Home › Add Data ›
Graphics Layer** adds a layer for [drawn graphics](cartography.md#graphics), and **Insert ›
Layer Templates** adds empty layers with ready-made schemas to draw into.

Vector coordinates are WGS 84 longitude/latitude and are displayed in Web Mercator. GeoPackage,
GeoParquet and FlatGeobuf files in another coordinate system (any EPSG code) are converted as they
are read. Rasters keep
their own coordinate system (any EPSG code, such as UTM zones or national grids) and are
reprojected on the fly as they draw.

## Formats through GDAL

When [GDAL](https://gdal.org) is installed (its `ogr2ogr` and `ogrinfo` programs on the `PATH`,
or the folder holding them in `RUSTGIS_GDAL_BIN`; GDAL 3.7 or later), the desktop app also opens
the vector formats GDAL reads: GML, DXF, File Geodatabases, MapInfo, OpenStreetMap
and more. RustGIS doesn't link to GDAL: each layer is converted once, by `ogr2ogr`, into a
FlatGeobuf with a spatial index in WGS 84 (GDAL reprojects it), kept in the cache folder
(`~/.cache/rustgis/gdal` on Linux, `~/Library/Caches/RustGIS/gdal` on macOS,
`%LOCALAPPDATA%\RustGIS\cache\gdal` on Windows, or `RUSTGIS_GDAL_CACHE`). Adding the file again
reuses the conversion until the file changes, and large layers stream like any large FlatGeobuf
(below). Every layer of the file is added; a layer mixing geometry types is
added as one layer per type. Projects remember the original file and convert it again if the
cached copy is gone.

## SQL queries and DuckDB databases

When [DuckDB](https://duckdb.org) is installed (the `duckdb` program on the `PATH`, or the folder
holding it in `RUSTGIS_DUCKDB_BIN`; DuckDB 1.5 or later), the desktop app adds the result of an SQL
query as a layer, with the `layer.addSql` command (Add Data From SQL in command search):

```json
{"sql": "SELECT name, geometry FROM read_parquet('places.parquet') WHERE country = 'FR'", "name": "French places"}
```

The query is one DuckDB `SELECT` (or `FROM …`, `WITH …`), with [DuckDB
Spatial](https://duckdb.org/docs/stable/core_extensions/spatial/overview)'s `ST_` functions: it can
read Parquet, CSV, JSON and GeoParquet files, anything `ST_Read` reads, and web addresses (`https://`,
`s3://`), so a bounding box of [Overture Maps](https://overturemaps.org) is one query:

```sql
SELECT id, names.primary AS name, geometry
FROM read_parquet('s3://overturemaps-us-west-2/release/2026-09-23.1/theme=places/type=place/*', hive_partitioning = 1)
WHERE bbox.xmin BETWEEN -83.95 AND -83.89 AND bbox.ymin BETWEEN 35.94 AND 35.98
```

The first geometry column becomes the layer's geometry; other columns become its attributes (types
RustGIS doesn't read, such as dates and lists, as text). A geometry that says its coordinate system
is converted to longitude/latitude; give `"crs"` (such as `EPSG:2263`) for one that doesn't. A
result mixing geometry types becomes a layer per type, and rows without a geometry are left out.
`"database"` opens a DuckDB database (read only) so the query names its tables plainly; the
database is trusted like the query, since its views run as part of it.

**Home › Add Data** opens a DuckDB database file (`.duckdb`, `.ddb`): every table with a geometry
column becomes a layer (`"layer"` picks one).

RustGIS doesn't link to DuckDB: it runs `duckdb`, which writes the result as GeoParquet in the
cache folder (`~/.cache/rustgis/duckdb` on Linux, `~/Library/Caches/RustGIS/duckdb` on macOS,
`%LOCALAPPDATA%\RustGIS\cache\duckdb` on Windows, or `RUSTGIS_DUCKDB_CACHE`), sorted along a
Hilbert curve with a bounding-box covering, so large results stream like any large GeoParquet
(below). DuckDB Spatial is installed the first time (a download from DuckDB's extension
repository). A database table is converted once and reused until the file changes; a query runs
afresh each time. Projects convert a table again if the cached result is gone, but never run a
query when they open: run it again. A database is read on its own, shut out of other files and web
addresses, so a view in it that reaches elsewhere fails rather than reading them.

A query can read any file the app can and fetch any web address, so the [AI
Assistant](assistant.md) asks before it runs one, and plugins need both the files and network
permissions.

## Geocoding addresses

**Home › Add Data › Geocode Addresses…** (`layer.geocode`) turns a table's addresses into points:
a CSV of addresses without coordinates is added as a standalone table, then each row's address
fields (`"address"`: a field, or several such as street, city and country, joined with commas)
are looked up, each distinct address once. The result is a new point layer holding every field
plus `geocoded`, what the geocoder called the place; rows that weren't found stay in its table
without a point.

The geocoder is OpenStreetMap's public [Nominatim](https://nominatim.org) by default, or
[Photon](https://photon.komoot.io) (`"provider": "photon"`). Their usage policies allow light
use, so RustGIS sends them at most one request a second and 100 addresses a run; give `"url"` of
a Nominatim or Photon server of your own for more (up to 10,000, with no wait between requests).
In the app the requests go out in the background, one at a time, and the layer is added when the
last answer lands; `layer.geocode` on the command line waits for them.

```sh
rustgis-cli run --cmd 'layer.addData={"path":"stores.csv"}' --cmd 'layer.geocode={"table":"stores","address":["street","city","country"]}'
```

The addresses are sent to the geocoder's server, so the [AI Assistant](assistant.md) asks before
it geocodes, and plugins need the network permission.

## Large files

GeoPackage, GeoParquet and FlatGeobuf files with more than 100,000 features are read in place
rather than loaded whole, so files far larger than memory open in a moment. The layer reads the
features in view, using the file's own spatial index (the GeoPackage R-tree, the FlatGeobuf
packed R-tree, GeoParquet row-group bounding boxes), in the background as the map moves. When a
view holds more than 250,000 features (100,000 in the web app), the layer draws an evenly spread
sample of them and its tooltip in Layers says so: zoom in to see them all. Smaller files load
whole; `layer.addData` with `"stream": true` or `false` overrides the choice.

A streamed layer works like any other for symbology, labels, pop-ups, selection and the
attribute table (which lists the features read for the view). Geoprocessing tools read the whole
file. Edits are refused, since the features are read again as the view moves: right-click the
layer in Layers and choose **Load All Features** (`layer.loadAll`) to load it into memory and
edit it. Projects save the layer's file path, not its features, and read the file again when they
open; a browser upload is kept in the project.

| File | Features | A view read |
| --- | --- | --- |
| GeoParquet, 0.1° grid (50 MB) | 1.6 million polygons | 183,000 in 0.15 s |
| FlatGeobuf, the same grid (335 MB) | 1.6 million polygons | 183,000 in 0.11 s |
| GeoPackage, the same grid (344 MB) | 1.6 million polygons | 183,000 in 0.18 s |
| GeoParquet, global grid (190 MB) | 6.5 million polygons | a sample of 136,000 in 0.27 s |

## Rasters

![A shaded-relief elevation raster of the Sierra Nevada with the Style tab open](https://assets.rustgis.app/images/raster.webp)

Add a GeoTIFF or an ASCII grid with **Add Data**, by dropping it on the window, or in the web app
by picking it. An ASCII grid takes its coordinate system from a `.prj` file next to it (as a Shapefile does);
otherwise, and always in the web app, which only sees the one file, it must be in
longitude/latitude degrees. Exporting a raster opened from an overview writes that overview.
GeoTIFFs may be regular or cloud-optimised: grey, RGB(A), palette (coloured by the file's colour
table, as land cover such as the NLCD is) or multiband, of any pixel type, tiled or striped, and
compressed with Deflate, LZW, PackBits or JPEG. The coordinate system is an EPSG code, or the
file's own projection described in its GeoKeys (Albers, Lambert conformal conic, transverse
Mercator, Mercator, Lambert azimuthal, stereographic, equirectangular and others), which an
exported GeoTIFF keeps. A raster too large to hold in memory (more than 120 million samples, 32
million in the web app) opens from its largest overview that fits; one without overviews opens as
an overview made as it is read, a strip or tile at a time (every second, fourth… pixel each way),
so reading it takes as long as decoding the whole file once.

With a raster layer selected in Layers, the **Style** and **Data** tabs show its groups:

| Tab › Group | Controls |
| --- | --- |
| Style › Visibility Range, Effects | Scale range, transparency |
| Style › Rendering | **Symbology**: Stretch, RGB, Classify, Unique Values, Colormap, Shaded Relief; **Stretch Type**: None, Minimum Maximum, Percent Clip, Standard Deviation, Histogram Equalize; **DRA** (stretch the pixels in view); **Resampling** (nearest or bilinear); **Band Combination** (natural colour, colour infrared, or any bands) |
| Style › Enhancement | Brightness, contrast, gamma |
| Data | Raster information (size, bands, statistics, coordinate system) and Export Raster (GeoTIFF) |

Clicking a raster with the Explore tool shows its pixel values. Projects remember a raster's file
path and reopen it; rasters added in the browser are stored inside the project.

## Project items

RustGIS saves pieces of a project as files you can share and reuse:

| Item | Extension | Made with | Opened with |
| --- | --- | --- | --- |
| Map file (a map with its data) | `.rgismap` | Publish › Save As › Map File | Data › Import |
| Layer file (a layer with its symbology) | `.rgislyr` | Publish › Save As › Layer File | Add Data |
| Styles (saved symbology) | `.rgisstyle` | Style › Styles › New Style, then Publish › Save As › Style Items | Style › Styles › Add Style File |
| Project template (the setup without features) | `.rgistemplate` | Data › Templates, or Publish › Save As | Data › Templates › Open Custom Template |
| Project package (everything, rasters included) | `.rgispkg` | Publish › Package › Project | Project › Open |
| Map package / mobile map package | `.rgismpk` | Publish › Package › Map or Mobile Map | Data › Import |
| Results package (geoprocessing history and outputs) | `.rgisgpk` | Publish › Package › Results | any zip tool |
| Task (guided command steps) | `.rgistask` | Data › Task, Publish › Save As › Task Item | Data › Task › Import Task |
| Layout | `.rgislayout` | Publish › Save Layout File | Data › New Publish › Import Layout |

Packages are zip files. A mobile map package keeps only the visible layers and simplifies their
geometry, for phones and tablets. Styles can also be imported from the layers of a layer file,
map file or project. Data › Layer Templates adds empty layers with ready-made schemas (places,
routes, areas, observations…) to draw into. Add Folder connects a folder to the Browser, which
also lists the project's layouts, styles and tasks. A task's steps each run a command; New Task
From History turns the geoprocessing history into a task you can replay.
