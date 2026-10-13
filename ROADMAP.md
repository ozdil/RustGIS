# Roadmap

Many features are first versions: there are no full 3D scenes or enterprise portal sharing yet.

## Done
- Layered workspace, command registry with undo, project files (`.rgis`), `rustgis-cli`, the
  web app (WebAssembly), 15 interface languages; a ribbon laid out in the order of the work
  (Home, Data, Style, Edit, Analyze, View, Publish), start-up preferences, Check for Updates.
- Data: files by their web address; GeoJSON, CSV/TSV (x/y in any EPSG system, or WKT), Excel and OpenDocument spreadsheets,
  GeoRSS, LandXML, encoded polylines, Shapefile (zipped too, with the coordinate system in its
  `.prj`), GeoPackage, GeoParquet, FlatGeobuf, KML/KMZ (styles included) and GPX in, all of them
  in the web app too (large files read in place, a view at a time); the vector formats GDAL
  reads when it is installed, and SQL queries and DuckDB databases (DuckDB Spatial) when DuckDB
  is; GeoJSON, CSV, GeoPackage, GeoParquet, Shapefile, FlatGeobuf, KML/KMZ, GPX and WKT out;
  standalone tables; layer, map, style, template, task and package files.
- Rasters: GeoTIFF/COG and ASCII grids (and JPEG 2000, NetCDF, HDF, ERDAS Imagine… through GDAL),
  EPSG reprojection, stretch/RGB/classified/shaded-relief
  symbology, raster functions, indices, change detection, classification, georeferencing.
- Services: WMS, WMTS, WFS, OGC API Features and ArcGIS REST, listed from their capabilities.
- Maps: multiple maps, a 3D globe, projection presets and EPSG codes, bookmarks, web tile
  layers (XYZ/TMS/WMS, Mapbox Vector Tiles, MapLibre styles, PMTiles and MBTiles archives),
  worldviews.
- Cartography: single, unique values, graduated colours and proportional symbols; labels with
  placement and annotation; masking, display filters, aggregation, extrusion; graphics.
- Editing: create, move, rotate, scale, merge, split, explode; snapping, templates, elevation,
  topology; domains, subtypes, attribute rules, contingent values, relationships, archiving.
- Analysis: selection by attributes and location; overlay, proximity and summary tools; guided
  workflows (viewshed, line of sight, IDW, shortest path, suitability); ModelBuilder and
  scripts; the GeoLibre and Whitebox suite (1,000+ tools) natively and in the browser.
- Layouts (map frames, surrounds, map series, PDF), reports, presentations and notebooks.
- Sharing as open formats (web maps, OGC API Features collections, MapLibre styles, glTF
  scenes, locators), offline areas with sync, deep learning pixel classification, hosted
  imagery tiles.
- The AI Assistant (Anthropic, OpenAI, Gemini, OpenRouter, Ollama, OpenAI-compatible) in the
  app, the web app and `rustgis-cli ask`; an MCP server (`rustgis-cli mcp`) for AI agents such
  as Claude Code, Cursor and VS Code, with the documentation built in.
- Sandboxed WebAssembly plugins with a registry at plugins.rustgis.app.
- Desktop packages: Windows installers (setup.exe, .msi), a portable zip and a Store MSIX; a
  signed and notarized macOS DMG; Linux .deb, .rpm, AppImage and tarballs; Homebrew, the AUR,
  Fedora COPR and winget; a download mirror at downloads.rustgis.app.

## Next
- Editing: vertex editing (move, add and delete vertices of existing features).
- Analysis: spatial join, intersect and union, near, heat maps.
- 3D: full scenes with terrain, beyond the globe and 2.5D extrusion.
