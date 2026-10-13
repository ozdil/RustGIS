# Supported Data Formats

This page lists the data formats RustGIS reads and writes, how to add each one, and the main
limits. [Data](data.md) covers adding data, large files and rasters in more detail; [Maps and
Basemaps](maps.md#basemaps) covers web tile layers.

Most formats open in the desktop app and in the [web app](https://web.rustgis.app) alike. Those
that need C libraries (GDAL, DuckDB) or files on disk are desktop-only, as the tables say. The
web app reads GeoPackages without SQLite, with a small SQLite file reader written in Rust.

## Vector and tabular files

| Format | Extensions or source | How to add it | Notes |
| --- | --- | --- | --- |
| GeoJSON | `.geojson`, `.json` | Home › Add Data, or drop the file | Features with typed attributes; a `GeometryCollection` keeps its first member. |
| Delimited text | `.csv`, `.tsv`, `.txt` | Add Data, or Add Data › Delimited Text… to choose the columns | Comma, semicolon, tab or pipe separated (found from the header, or `"delimiter"`). Points from x/y columns (named like longitude/latitude or easting/northing, or chosen with `"x"` and `"y"`), or any geometry from a WKT/EWKT column (named like `wkt` or `geometry`, or `"wkt"`). Coordinates in another system are converted from `"crs"` (`EPSG:2263`, or WKT) or an EWKT `SRID=`; without either, coordinates outside longitude/latitude are pointed out. A file without coordinates opens as a standalone table. |
| Shapefile | `.shp` with `.dbf` (and `.shx`), or zipped (`.zip`) | Add Data, drop the file, or upload the zip in the web app | A zip may hold several Shapefiles, in folders, and other vector files (GeoJSON, CSV, GeoParquet, FlatGeobuf); each becomes a layer, at most 32 for the whole archive (Shapefiles first, so other files are left out when Shapefiles fill it). `.txt` files and macOS `__MACOSX` entries are passed over, and a member that can't be read is skipped and named in a notice. Converted to longitude/latitude from the coordinate system in its `.prj` (EPSG, Esri or OGC WKT; Web Mercator, UTM, State Plane and other common projections); without a `.prj` it must be in longitude/latitude. |
| KML / KMZ | `.kml`, `.kmz` | Add Data, drop the file, or upload it in the web app | A layer for each kind of geometry (points, lines, polygons), with each placemark's name, description, folder path and extended data. Styles are kept: the layer is drawn by unique values of its `style` field, with each KML style's line and fill colours, widths and icon colours (up to 64 styles). Multi-geometries and `gx:Track`s are read; ground overlays are counted, not drawn. |
| GPX | `.gpx` | Add Data, drop the file, or upload it in the web app | Waypoints, routes, tracks (one line per track, its segments as parts) and track points (with elevation and time) as separate layers. |
| Spreadsheets | `.xlsx`, `.xlsm`, `.xlsb`, `.xls`, `.ods` | Add Data (or Delimited Text… to choose the columns), drop the file, or upload it in the web app | One sheet at a time, read like delimited text: the first non-empty row names the columns, and x/y or WKT columns (found, or given with `"x"`, `"y"`, `"wkt"` and `"crs"`) make the geometry; a sheet without them is a table. The first sheet is added and the others named in a notice; `"layer"` picks one, and `info.fileLayers` lists them. Dates read as ISO 8601. |
| GeoRSS | `.rss`, `.atom`, `.georss`, or `.xml` | Add Data, drop the file, upload, or From URL… | RSS 2.0, RSS 1.0 and Atom feeds with GeoRSS Simple, GeoRSS GML or W3C Basic Geo places: points, lines and polygons layers with each item's title, link, description, date, id and category. |
| LandXML | `.landxml`, or `.xml` | Add Data, drop the file, or upload | Coordinate geometry points, parcels (polygons), alignments (lines; curves as chords) and TIN surfaces (a triangle per face, with its mean elevation), converted from the file's `epsgCode` or `"crs"`. |
| Encoded polylines | text | `layer.addEncodedPolyline` with `"polyline"` (one or a list) | Google's encoding, precision 5, or 6 (`"precision"`) as OSRM and Valhalla can give: a line layer. |
| GeoPackage | `.gpkg` | Add Data, or upload in the web app | Every feature table becomes a layer, up to 32 at once (the result counts the rest in `skippedCount` and names up to 100 of them in `skipped`); `"layer"` picks one, and `info.fileLayers` lists them all. Read in place by its R-tree; any EPSG system is reprojected. The web app reads the file with its own SQLite reader (tables scanned, the envelope in each geometry header tested first; no R-tree); UTF-16 databases aren't read there. |
| GeoParquet | `.parquet`, `.geoparquet` | Add Data, or upload in the web app | WKB geometry, with the CRS from the file's `geo` metadata. Row groups are pruned by the bbox covering. zstd-compressed files open on the desktop only; Brotli isn't read. |
| FlatGeobuf | `.fgb` | Add Data, or upload | Read in place by its packed R-tree. |
| Standalone tables | CSV, or any layer without geometry | Add Data | Listed under the map in Layers; join or relate them to layers. |

Files with more than 100,000 features (GeoPackage, GeoParquet, FlatGeobuf) are read in place, a view
at a time, rather than loaded whole: see [Large files](data.md#large-files). They draw, label,
identify and run through tools like any layer; **Load All Features** loads one to edit it.

## Formats read through GDAL

With [GDAL](https://gdal.org) installed (its `ogr2ogr` and `ogrinfo` on the `PATH`, GDAL 3.7 or
later), the desktop app also reads the vector formats below. Each layer is converted once into a
cached, indexed FlatGeobuf, so large ones stream too: see [Formats through
GDAL](data.md#formats-through-gdal).

| Format | Extensions or source | Notes |
| --- | --- | --- |
| Esri File Geodatabase | `.gdb` folder | Every feature class becomes a layer. |
| AutoCAD DXF | `.dxf` | Give `"crs"` (such as `EPSG:2263`) when the drawing doesn't say its coordinate system. |
| GML | `.gml` | |
| MapInfo | `.tab`, `.mif` | Keep the companion files together. |
| OpenStreetMap | `.osm`, `.osm.pbf` | |
| GeoJSON Lines, GMT | `.geojsonl`, `.geojsons`, `.gmt` | |

## Files on the web

**Home › Add Data › From URL…** (`layer.addUrl` with `"url"`) downloads a file by its `https://`
address and adds it like a file from this computer: GeoJSON, CSV, GeoParquet, FlatGeobuf,
GeoPackage, zipped Shapefiles, KML/KMZ, GPX or GeoTIFF/COG, up to 512 MB on the desktop and 256 MB
in the web app (where the server must allow requests from web pages, CORS). The format comes from
the address, or from the contents when the address has no extension (an API endpoint, say). The
app downloads in the background and adds the layer when the file arrives (to the project it was
started in: opening another meanwhile drops it). A `.pmtiles` address is added as a tile layer
and read a tile at a time instead.

`layer.addUrl` waits for the download, up to a minute, which suits the command line, MCP and the
desktop app's assistant; a larger file comes through the From URL… dialog, which downloads in the
background. In the web app, where nothing may wait, it adds `.pmtiles` addresses only; other files
come through the From URL… dialog (`ui.addUrl`). GeoJSON, CSV, KML and GPX files are read up to
256 MB; larger downloads (GeoParquet, FlatGeobuf, GeoPackage, GeoTIFF) up to the limits above.

## Map and feature services

**Home › Add Data › From Service…** connects to a service by its address, lists what it offers from
its capabilities, and adds a layer at a time (`service.layers` and `service.add` from scripts, the
command line and MCP).

| Service | What it lists | How a layer is added |
| --- | --- | --- |
| WMS 1.1.1 / 1.3.0 | Named layers offered in Web Mercator | A tile layer of GetMap images (PNG when the service offers it, else JPEG) |
| WMTS | Layers whose tile matrix set is Web Mercator's pyramid | A tile layer from the tile template (or KVP GetTile) |
| WFS 1.1 / 2.0 | Feature types | Their features downloaded as GeoJSON, in the output format the service names (up to 50,000) |
| OGC API Features | Collections | Their items downloaded as GeoJSON |
| ArcGIS REST | FeatureServer and MapServer layers; a MapServer's map; an ImageServer; a VectorTileServer | Layers queried as GeoJSON (up to the server's record limit); a map as its tile cache when it is Web Mercator's pyramid, else exported images; an ImageServer as exported images; a VectorTileServer as its MapLibre style |

The type is guessed from the address (`?SERVICE=`, `/wmts`, `/wfs`, `/collections`, `/rest/services`…) or
chosen. A service that asks for a token says so. In the web app the server must allow requests
from web pages (CORS).

## SQL queries and DuckDB databases

With [DuckDB](https://duckdb.org) installed (the `duckdb` program on the `PATH`, DuckDB 1.5 or
later), the desktop app adds the result of an SQL query as a layer, and opens DuckDB database files.
Results are written as cached GeoParquet, so large ones stream too: see [SQL queries and DuckDB
databases](data.md#sql-queries-and-duckdb-databases).

| Source | How to add it | Notes |
| --- | --- | --- |
| SQL query | `layer.addSql` with `"sql"` | One DuckDB `SELECT` with DuckDB Spatial: Parquet, CSV, GeoParquet and `ST_Read` files, web addresses (`https://`, `s3://`). Give `"crs"` for a geometry that doesn't say its coordinate system. |
| DuckDB database | `.duckdb`, `.ddb`: Add Data | Every table with a geometry column becomes a layer (`"layer"` picks one). Converted once, until the file changes. |

## Rasters

| Format | Extensions or source | How to add it | Notes |
| --- | --- | --- | --- |
| GeoTIFF / Cloud-optimised GeoTIFF | `.tif`, `.tiff` | Add Data, or upload | Regular or cloud-optimised, striped or tiled; grey, RGB(A), palette (with its colour table) or multiband. Any EPSG system, or the file's own projection from its GeoKeys (Albers, LCC, transverse Mercator…), reprojected as it draws. A large file opens from its overview, or one made as it is read when it has none. |
| ASCII grid | `.asc` (with an optional `.prj`) | Add Data, or upload | |
| Rasters GDAL reads | `.jp2`, `.img`, `.hgt`, `.dem`, `.dt0`–`.dt2`, `.nc`, `.hdf`, `.h5`, `.bil`, `.grd`, `.sdat`, `.kea`, `.ers`, `.rst`, `.ntf`, `.ecw`, `.sid`… | Add Data (desktop, with [GDAL](https://gdal.org) installed: `gdal_translate`, `gdalinfo` and `gdalwarp`) | Converted once to a cloud-optimised GeoTIFF in the cache, then read as one. A NetCDF or HDF file's variables are its subdatasets: the first is added, the others are named in a notice, and `"layer"` picks one. A file that doesn't say its coordinate system is read as WGS 84 when its coordinates look like longitude/latitude (0–360° longitudes are moved to -180–180°); otherwise give `"crs"`. |

## Tiles and web services

| Format | Extensions or source | How to add it | Notes |
| --- | --- | --- | --- |
| Raster tiles | XYZ/TMS template (`{z}/{x}/{y}`, `{-y}`, `{s}`, `{q}`), WMS (`{bbox-epsg-3857}`) | Home › Add › Basemap › Add Tile Layer | PNG, JPEG and WebP. |
| Vector tiles | Mapbox Vector Tiles (`.pbf`, `.mvt`) template, or TileJSON | Add Tile Layer | Drawn in a built-in style that knows the OpenMapTiles, Protomaps and Shortbread schemas. |
| MapLibre styles | `style.json` | Add Tile Layer | The style's vector and raster sources, sprite, expressions and zoom functions. |
| PMTiles | `.pmtiles`: a web address, or a file on the desktop | Add Tile Layer (or its **Tile Archive File…** button), Add Data, or drop the file | Raster or vector, as the archive says. Read a tile at a time by byte ranges (the server must allow them, and CORS in the web app). A vector archive is drawn in the built-in style when it is a basemap, otherwise with each of its layers in its own colour. |
| MBTiles | `.mbtiles`: a file | Add Data, drop the file, Add Tile Layer's **Tile Archive File…** button, or upload in the web app | Raster (PNG, JPEG, WebP) or vector tiles, as its metadata says, drawn as PMTiles archives are. The desktop reads the file in place, a tile at a time. The web app reads an upload into memory (the plain `tiles` table and the deduplicated layouts of mbutil, TileMill and Planetiler) and keeps it for the session: a project saved there asks for the file again. |

## Writing

| Format | How to write it | Notes |
| --- | --- | --- |
| GeoJSON, CSV | Data › Export (`share.exportLayer`) | The Export Features dialog picks the format; the file is saved in it whatever extension is typed. In the web app it downloads (any format below but GeoPackage; a Shapefile as one zip). A streamed layer writes its whole source (as every format below does). |
| GeoPackage | Data › Export to `.gpkg` (desktop) | One feature table (an attributes table for a layer without geometry) in WGS 84, with an R-tree spatial index and the GeoPackage 1.4 triggers that keep it current when other software edits the file. Feature ids are kept. |
| GeoParquet | Data › Export to `.parquet` | GeoParquet 1.1: WKB geometry in WGS 84, Snappy compression, row groups of 65,536 features and a `bbox` covering column so readers can skip rows by extent. |
| Shapefile | Data › Export to `.shp` (or `.zip` for the files zipped) | Writes `.shp`, `.shx`, `.dbf`, `.prj` (WGS 84) and `.cpg` (UTF-8). Field names are cut to 10 characters (made unique), text to 254 bytes, and numbers too wide for a dBase field are left empty; features without geometry, and lines of a single point, are left out (and reported). |
| FlatGeobuf | Data › Export to `.fgb` | With a spatial index; features without geometry are left out (and reported). |
| KML, KMZ, GPX | Data › Export to `.kml`, `.kmz` or `.gpx` | Placemarks named by a `name` field, if there is one; features without geometry are left out. |
| KML, GPX, CSV with WKT | Analyze › Workflows › Data Interop | |
| GeoTIFF | Data › Export | |
| PNG | Publish › Export Map (`share.exportMap`) | At any DPI, with labels, scale bar and tile credits. |
| PDF | Layouts, reports and presentations | |
| Wavefront OBJ, glTF | Publish › Export To 3D, Share As › Web Scene | Polygons extruded by a field. |
| Web map, OGC API Features, MapLibre style | Publish › Share As | Open-format files that need no server: see [Sharing](sharing.md). |
| Raster tiles | Analyze › Create Hosted Imagery | |

## RustGIS files

| File | Extension | Notes |
| --- | --- | --- |
| Project | `.rgis` (older `.rgisx` open too) | Saves streamed layers' file paths, not their features; a browser upload is kept inside. |
| Map, layer, style, template, task and layout files | `.rgismap`, `.rgislyr`, `.rgisstyle`, `.rgistemplate`, `.rgistask`, `.rgislayout` | See [Project items](data.md#project-items). |
| Project package | `.rgispkg` | Everything, rasters included. |
| Plugin | `.rgplugin` | See [Plugins](../plugins.md). |
