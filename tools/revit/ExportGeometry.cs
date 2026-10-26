// Revit macro: export lightweight triangulated geometry of chosen elements to one OBJ file.
//
// Each element (or each family type) becomes one named object in the OBJ. The name is the value of NAME_PARAMETER,
// so the solid can be matched to a node with the same value. Units: metres, Z up.
//
// Setup: Manage ▸ Macro Manager ▸ Application tab ▸ Module (C#). In the module that opens, keep the generated
// namespace, class header and startup code, add the `using` lines below to the top, and paste everything between
// the two "paste" markers inside the ThisApplication class. Build, then run ExportGeometry from the Macro Manager.
// Written in plain C# 5 so it builds in older and newer macro editors (Revit 2022 and later).

using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using Autodesk.Revit.DB;
using Autodesk.Revit.UI;

// ───────────────────────────── paste from here ─────────────────────────────

        // ── settings ──

        // Which elements: a parameter name and the values to keep. Besides any parameter name, these work too:
        // "Workset", "Category", "Family", "Type", "Level". Leave FILTER_PARAMETER empty to take every element.
        const string FILTER_PARAMETER = "Workset";
        static readonly string[] FILTER_VALUES = { "MEP - Mechanical" };
        // true: keep elements whose value contains one of the values (ignoring case); false: equal to one of them
        const bool FILTER_CONTAINS = false;

        // What each solid is called in the file (to match a node in Graphsight): "Id", "UniqueId", or any parameter
        // name such as "Mark". Elements without a value are called by their Id.
        const string NAME_PARAMETER = "Id";

        // false: one solid per element, at its place in the model.
        // true:  one solid per family type, in the family's own coordinates (a library of shapes);
        //        each is called "Family : Type". System families (walls, ducts …) are skipped in this mode.
        const bool PER_TYPE = false;

        // Only elements visible in the active view; and if elements are selected, only those.
        const bool ONLY_ACTIVE_VIEW = true;
        const bool USE_SELECTION = true;

        // Shared coordinates (survey point) instead of Revit's internal origin.
        const bool SHARED_COORDINATES = false;
        // Revit works in feet: 0.3048 writes metres, 304.8 millimetres, 1.0 feet.
        const double SCALE = 0.3048;
        // Lightweight: Coarse family geometry, and the coarsest triangulation (0 coarse … 1 fine).
        const ViewDetailLevel DETAIL = ViewDetailLevel.Coarse;
        const double TRIANGULATION = 0.0;
        // Coordinates are rounded to this many decimals (4 decimals of a metre = 0.1 mm).
        const int DECIMALS = 4;

        // ── the macro ──

        public void ExportGeometry()
        {
            UIDocument uidoc = this.ActiveUIDocument;
            Document doc = uidoc.Document;

            List<Element> elements = PickElements(uidoc);
            if (elements.Count == 0)
            {
                TaskDialog.Show("Export geometry", "No elements match the settings.");
                return;
            }

            string folder = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Desktop), "Revit geometry");
            Directory.CreateDirectory(folder);
            string file = Path.Combine(folder, Safe(doc.Title) + (PER_TYPE ? "_types" : "") + "_" + DateTime.Now.ToString("yyyyMMdd_HHmm") + ".obj");

            Options opt = new Options();
            opt.DetailLevel = DETAIL;
            opt.ComputeReferences = false;
            opt.IncludeNonVisibleObjects = false;

            Transform toFile = SHARED_COORDINATES ? doc.ActiveProjectLocation.GetTotalTransform().Inverse : Transform.Identity;

            int objects = 0, triangles = 0, vertexBase = 1;
            HashSet<string> doneTypes = new HashSet<string>();
            HashSet<string> usedNames = new HashSet<string>();

            using (StreamWriter w = new StreamWriter(file, false, new UTF8Encoding(false)))
            {
                w.WriteLine("# Revit geometry from " + doc.Title);
                w.WriteLine("# units: " + (Math.Abs(SCALE - 0.3048) < 1e-9 ? "metres" : "scale " + Num(SCALE) + " x feet") + ", Z up, "
                    + (PER_TYPE ? "one object per family type in family coordinates" : (SHARED_COORDINATES ? "shared coordinates" : "internal coordinates")));
                w.WriteLine("# object names: " + (PER_TYPE ? "Family : Type" : NAME_PARAMETER));

                foreach (Element e in elements)
                {
                    List<XYZ> verts = new List<XYZ>();
                    List<int> tris = new List<int>();
                    string name;

                    if (PER_TYPE)
                    {
                        FamilyInstance fi = e as FamilyInstance;
                        if (fi == null || fi.Symbol == null) continue;
                        string key = fi.Symbol.Id.ToString();
                        if (doneTypes.Contains(key)) continue;
                        // the family's own geometry sits in a GeometryInstance; instances cut by a host do not have one
                        GeometryElement ge = e.get_Geometry(opt);
                        GeometryInstance gi = ge == null ? null : FirstInstance(ge);
                        if (gi == null) continue;
                        Gather(gi.GetSymbolGeometry(), Transform.Identity, verts, tris);
                        if (tris.Count == 0) continue;
                        doneTypes.Add(key);
                        name = fi.Symbol.FamilyName + " : " + fi.Symbol.Name;
                    }
                    else
                    {
                        GeometryElement ge = e.get_Geometry(opt);
                        if (ge == null) continue;
                        Gather(ge, toFile, verts, tris);
                        if (tris.Count == 0) continue;
                        name = ValueOf(doc, e, NAME_PARAMETER);
                        if (string.IsNullOrEmpty(name)) name = e.Id.ToString();
                    }

                    // two solids with the same name would merge on import: keep names unique
                    string unique = name;
                    for (int k = 2; usedNames.Contains(unique); k++) unique = name + " #" + k;
                    usedNames.Add(unique);

                    w.WriteLine("o " + unique.Replace("\r", " ").Replace("\n", " "));
                    foreach (XYZ p in verts)
                        w.WriteLine("v " + Num(p.X) + " " + Num(p.Y) + " " + Num(p.Z));
                    for (int t = 0; t < tris.Count; t += 3)
                        w.WriteLine("f " + (tris[t] + vertexBase) + " " + (tris[t + 1] + vertexBase) + " " + (tris[t + 2] + vertexBase));
                    vertexBase += verts.Count;
                    objects++;
                    triangles += tris.Count / 3;
                }
            }

            TaskDialog.Show("Export geometry", objects + " solids, " + triangles + " triangles\n\n" + file);
        }

        // ── which elements ──

        List<Element> PickElements(UIDocument uidoc)
        {
            Document doc = uidoc.Document;
            List<Element> picked = new List<Element>();
            ICollection<ElementId> sel = uidoc.Selection.GetElementIds();
            IEnumerable<Element> source;
            if (USE_SELECTION && sel.Count > 0)
            {
                List<Element> fromSel = new List<Element>();
                foreach (ElementId id in sel) fromSel.Add(doc.GetElement(id));
                source = fromSel;
            }
            else if (ONLY_ACTIVE_VIEW)
                source = new FilteredElementCollector(doc, doc.ActiveView.Id).WhereElementIsNotElementType();
            else
                source = new FilteredElementCollector(doc).WhereElementIsNotElementType();

            foreach (Element e in source)
            {
                if (e == null || e.Category == null || e.ViewSpecific) continue;
                if (e.Category.CategoryType != CategoryType.Model) continue;
                if (e is RevitLinkInstance || e is ImportInstance) continue;
                if (!string.IsNullOrEmpty(FILTER_PARAMETER) && FILTER_VALUES.Length > 0 && !Matches(ValueOf(doc, e, FILTER_PARAMETER))) continue;
                picked.Add(e);
            }
            return picked;
        }

        static bool Matches(string value)
        {
            if (value == null) value = "";
            foreach (string want in FILTER_VALUES)
            {
                if (FILTER_CONTAINS ? value.IndexOf(want, StringComparison.OrdinalIgnoreCase) >= 0
                                    : string.Equals(value, want, StringComparison.OrdinalIgnoreCase))
                    return true;
            }
            return false;
        }

        /// A parameter's value as text (looked up on the element, then on its type).
        static string ValueOf(Document doc, Element e, string name)
        {
            if (string.IsNullOrEmpty(name)) return "";
            ElementType type = doc.GetElement(e.GetTypeId()) as ElementType;
            switch (name.ToLowerInvariant())
            {
                case "id": return e.Id.ToString();
                case "uniqueid": return e.UniqueId;
                case "category": return e.Category != null ? e.Category.Name : "";
                case "family": return type != null ? type.FamilyName : "";
                case "type": return type != null ? type.Name : "";
                case "workset":
                    return doc.IsWorkshared ? doc.GetWorksetTable().GetWorkset(e.WorksetId).Name : "";
                case "level":
                    Element lvl = doc.GetElement(e.LevelId);
                    if (lvl != null) return lvl.Name;
                    break; // some families keep their level in a parameter called "Level"
            }
            Parameter p = e.LookupParameter(name);
            if ((p == null || !p.HasValue) && type != null) p = type.LookupParameter(name);
            if (p == null || !p.HasValue) return "";
            switch (p.StorageType)
            {
                case StorageType.String:
                    return p.AsString() ?? "";
                case StorageType.ElementId:
                    Element other = doc.GetElement(p.AsElementId());
                    return other != null ? other.Name : p.AsElementId().ToString();
                default:
                    string shown = p.AsValueString();
                    return shown ?? (p.StorageType == StorageType.Integer ? p.AsInteger().ToString(CultureInfo.InvariantCulture) : Num(p.AsDouble()));
            }
        }

        // ── geometry ──

        static GeometryInstance FirstInstance(GeometryElement ge)
        {
            foreach (GeometryObject o in ge)
            {
                GeometryInstance gi = o as GeometryInstance;
                if (gi != null) return gi;
            }
            return null;
        }

        /// Every solid and mesh in `ge` (nested families too) as triangles, through `tf`, scaled, with shared corners
        /// written once.
        static void Gather(GeometryElement ge, Transform tf, List<XYZ> verts, List<int> tris)
        {
            Dictionary<string, int> index = new Dictionary<string, int>();
            GatherInto(ge, tf, verts, tris, index);
        }

        static void GatherInto(GeometryElement ge, Transform tf, List<XYZ> verts, List<int> tris, Dictionary<string, int> index)
        {
            foreach (GeometryObject o in ge)
            {
                Solid solid = o as Solid;
                if (solid != null)
                {
                    if (solid.Faces.Size == 0 || solid.Volume < 1e-9) continue; // empty or invisible helper solids
                    foreach (Face face in solid.Faces)
                    {
                        Mesh m = face.Triangulate(TRIANGULATION);
                        if (m != null) AddMesh(m, tf, verts, tris, index);
                    }
                    continue;
                }
                Mesh mesh = o as Mesh;
                if (mesh != null)
                {
                    AddMesh(mesh, tf, verts, tris, index);
                    continue;
                }
                GeometryInstance inst = o as GeometryInstance;
                if (inst != null)
                    GatherInto(inst.GetInstanceGeometry(), tf, verts, tris, index);
            }
        }

        static void AddMesh(Mesh m, Transform tf, List<XYZ> verts, List<int> tris, Dictionary<string, int> index)
        {
            for (int i = 0; i < m.NumTriangles; i++)
            {
                MeshTriangle t = m.get_Triangle(i);
                int a = Vertex(t.get_Vertex(0), tf, verts, index);
                int b = Vertex(t.get_Vertex(1), tf, verts, index);
                int c = Vertex(t.get_Vertex(2), tf, verts, index);
                if (a == b || b == c || a == c) continue; // collapsed by rounding
                tris.Add(a);
                tris.Add(b);
                tris.Add(c);
            }
        }

        static int Vertex(XYZ p, Transform tf, List<XYZ> verts, Dictionary<string, int> index)
        {
            XYZ q = tf.OfPoint(p);
            XYZ s = new XYZ(Math.Round(q.X * SCALE, DECIMALS), Math.Round(q.Y * SCALE, DECIMALS), Math.Round(q.Z * SCALE, DECIMALS));
            string key = Num(s.X) + "," + Num(s.Y) + "," + Num(s.Z);
            int k;
            if (index.TryGetValue(key, out k)) return k;
            k = verts.Count;
            verts.Add(s);
            index.Add(key, k);
            return k;
        }

        static string Num(double d)
        {
            return d.ToString("0.####", CultureInfo.InvariantCulture);
        }

        static string Safe(string s)
        {
            foreach (char c in Path.GetInvalidFileNameChars()) s = s.Replace(c, '_');
            return s;
        }

// ───────────────────────────── paste up to here ─────────────────────────────
