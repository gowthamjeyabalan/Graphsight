// Revit macro: export lightweight solids of chosen elements to one OBJ file, for Graphsight.
//
// Running ExportGeometry opens a window: pick the elements by any parameter and its values (workset, category,
// family, type, level or any other parameter), the parameter each solid is named by (Graphsight matches this name
// to a node attribute) and the detail level, then where to save the .obj. Each solid is written as a shape only,
// centred on 0, 0, 0, in metres with Z up: the node's own coordinates say where it goes. With "one solid per name"
// (on by default) only the first element of each name is written, so e.g. one solid per family serves all its nodes.
//
// Setup (Revit 2025 and later): Manage > Macro Manager > Application tab > Module (C#). Replace the module's
// ThisApplication.cs with this file (keep the AddInId line Revit generated for your module), build, then run
// ExportGeometry from the Macro Manager.

using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Text;
using Autodesk.Revit.DB;
using Autodesk.Revit.UI;
using TaskDialog = Autodesk.Revit.UI.TaskDialog;
using System.Windows.Forms;
using Form = System.Windows.Forms.Form;
using ComboBox = System.Windows.Forms.ComboBox;
using CheckBox = System.Windows.Forms.CheckBox;
using Label = System.Windows.Forms.Label;
using Button = System.Windows.Forms.Button;

namespace ExportGeometry
{
   [Autodesk.Revit.Attributes.Transaction(Autodesk.Revit.Attributes.TransactionMode.Manual)]
   [Autodesk.Revit.DB.Macros.AddInId("1FBCE3D2-16D9-4D48-9207-E846BD64C55B")]
   public partial class ThisApplication
   {
      private void Module_Startup(object? sender, EventArgs e)
      {
      }

      private void Module_Shutdown(object? sender, EventArgs e)
      {
      }

      // ------------------------------------------------------------------
      // SETTINGS that do not change from run to run
      // (which elements, how solids are named and the detail are chosen in the window each time).
      // Every solid is written as a shape only, centred on 0, 0, 0: Graphsight puts it on its node.
      // ------------------------------------------------------------------

      // Only elements visible in the active view; and if elements are selected, only those.
      private const bool ONLY_ACTIVE_VIEW = true;
      private const bool USE_SELECTION = true;

      // Revit works in feet: 0.3048 = metres, 304.8 = millimetres, 1.0 = feet.
      private const double SCALE = 0.3048;
      private const double TRIANGULATION = 0.0;   // 0 coarse ... 1 fine
      private const int DECIMALS = 4;

      // Parameter names are collected from at most this many elements (enough to find them all, and quick).
      private const int PARAMETER_SAMPLE = 3000;

      // Names that are not Revit parameters but work everywhere a parameter name does.
      private static readonly string[] SPECIAL = { "Id", "UniqueId", "Category", "Family", "Type", "Family : Type", "Workset", "Level" };

      // What the window hands back.
      private class Choice
      {
         public string FilterParameter = "";            // "" = every element
         public HashSet<string> FilterValues = new HashSet<string>();
         public string NameParameter = "Id";
         public bool OnePerName = true;                  // only the first element of each name is written
         public ViewDetailLevel Detail = ViewDetailLevel.Coarse;
      }

      // ------------------------------------------------------------------
      // MACRO ENTRY POINT  (run this one from Macro Manager)
      // Choose the elements and the naming in the window, then where to save the .obj.
      // ------------------------------------------------------------------
      public void ExportGeometry()
      {
         try
         {
            UIDocument uidoc = this.ActiveUIDocument;
            Document doc = uidoc.Document;

            List<Element> candidates = Candidates(uidoc);
            if (candidates.Count == 0)
            {
               TaskDialog.Show("Export geometry",
                  "There are no model elements to export.\n\nOnly active view: " + ONLY_ACTIVE_VIEW + "\nUse selection: " + USE_SELECTION);
               return;
            }

            Choice? choice = AskChoice(doc, candidates);
            if (choice == null)
               return;

            List<Element> elements = candidates.Where(e => Keep(doc, e, choice)).ToList();
            if (elements.Count == 0)
            {
               TaskDialog.Show("Export geometry", "No elements have the chosen values.");
               return;
            }

            string? file = null;
            using (SaveFileDialog dlg = new SaveFileDialog())
            {
               dlg.Title = "Save geometry as OBJ";
               dlg.Filter = "OBJ file|*.obj";
               dlg.FileName = Safe(doc.Title) + "_" + Safe(choice.NameParameter) + "_" + DateTime.Now.ToString("yyyyMMdd_HHmm") + ".obj";
               dlg.InitialDirectory = Environment.GetFolderPath(Environment.SpecialFolder.Desktop);
               if (dlg.ShowDialog() != DialogResult.OK)
                  return;
               file = dlg.FileName;
            }

            int objects, triangles;
            WriteObj(doc, elements, choice, file, out objects, out triangles);

            TaskDialog.Show("Export geometry",
               objects + " solids, " + triangles + " triangles\n" +
               "Named by: " + choice.NameParameter + "\n" +
               "Each solid is centred on 0, 0, 0 (shape and size only): Graphsight puts it on its node.\n\n" + file);
         }
         catch (Exception ex)
         {
            TaskDialog.Show("Export geometry", "Export failed:\n" + ex.Message);
         }
      }

      // ------------------------------------------------------------------
      // The window: which elements, what to call them, how detailed
      // ------------------------------------------------------------------
      private Choice? AskChoice(Document doc, List<Element> candidates)
      {
         List<string> names = ParameterNames(doc, candidates);
         const string EVERY = "(every element)";

         Form f = new Form();
         f.Text = "Export geometry for Graphsight";
         f.FormBorderStyle = FormBorderStyle.FixedDialog;
         f.StartPosition = FormStartPosition.CenterScreen;
         f.MinimizeBox = false;
         f.MaximizeBox = false;
         f.ClientSize = new System.Drawing.Size(460, 560);
         f.Font = new System.Drawing.Font("Segoe UI", 9f);

         int y = 14;
         Label Add(string text, int top, bool bold = false)
         {
            Label l = new Label();
            l.Text = text;
            l.AutoSize = true;
            l.Location = new System.Drawing.Point(14, top);
            if (bold) l.Font = new System.Drawing.Font(f.Font, System.Drawing.FontStyle.Bold);
            f.Controls.Add(l);
            return l;
         }

         // which elements
         Add("Which elements", y, true);
         y += 24;
         Add("Export the elements whose", y + 3);
         ComboBox cbFilter = new ComboBox();
         cbFilter.DropDownStyle = ComboBoxStyle.DropDownList;
         cbFilter.Location = new System.Drawing.Point(180, y);
         cbFilter.Width = 266;
         cbFilter.Items.Add(EVERY);
         foreach (string n in names) cbFilter.Items.Add(n);
         cbFilter.SelectedIndex = 0;
         f.Controls.Add(cbFilter);
         y += 32;
         Add("is one of these values (tick them):", y);
         y += 22;
         CheckedListBox lbValues = new CheckedListBox();
         lbValues.Location = new System.Drawing.Point(14, y);
         lbValues.Size = new System.Drawing.Size(432, 190);
         lbValues.CheckOnClick = true;
         f.Controls.Add(lbValues);
         y += 196;
         Button bAll = new Button();
         bAll.Text = "All";
         bAll.Location = new System.Drawing.Point(14, y);
         bAll.Width = 70;
         Button bNone = new Button();
         bNone.Text = "None";
         bNone.Location = new System.Drawing.Point(90, y);
         bNone.Width = 70;
         f.Controls.Add(bAll);
         f.Controls.Add(bNone);
         Label count = Add("", y + 5);
         count.Location = new System.Drawing.Point(172, y + 5);
         y += 40;

         // naming
         Add("What Graphsight matches", y, true);
         y += 24;
         Add("Name each solid by", y + 3);
         ComboBox cbName = new ComboBox();
         cbName.DropDownStyle = ComboBoxStyle.DropDownList;
         cbName.Location = new System.Drawing.Point(180, y);
         cbName.Width = 266;
         foreach (string n in names) cbName.Items.Add(n);
         cbName.SelectedItem = "Id";
         f.Controls.Add(cbName);
         y += 30;
         Label nameHelp = Add("Use the same parameter as the column you match by in Graphsight.", y);
         nameHelp.ForeColor = System.Drawing.SystemColors.GrayText;
         y += 24;
         CheckBox chkOne = new CheckBox();
         chkOne.Text = "One solid per name: only the first element of each name is written" + Environment.NewLine + "(every node with that name gets the same solid)";
         chkOne.AutoSize = true;
         chkOne.Checked = true;
         chkOne.Location = new System.Drawing.Point(14, y);
         f.Controls.Add(chkOne);
         y += 46;

         // detail and coordinates
         Add("Geometry", y, true);
         y += 24;
         Add("Detail level", y + 3);
         ComboBox cbDetail = new ComboBox();
         cbDetail.DropDownStyle = ComboBoxStyle.DropDownList;
         cbDetail.Location = new System.Drawing.Point(180, y);
         cbDetail.Width = 266;
         cbDetail.Items.AddRange(new object[] { "Coarse (lightest; many families are simple boxes)", "Medium", "Fine (true shapes, heaviest)" });
         cbDetail.SelectedIndex = 0;
         f.Controls.Add(cbDetail);
         y += 32;
         Label shapeHelp = Add("Solids are written as shapes only, centred on 0, 0, 0, in metres.", y);
         shapeHelp.ForeColor = System.Drawing.SystemColors.GrayText;
         y += 34;

         Button ok = new Button();
         ok.Text = "Export…";
         ok.DialogResult = DialogResult.OK;
         ok.Location = new System.Drawing.Point(266, y);
         ok.Width = 86;
         Button cancel = new Button();
         cancel.Text = "Cancel";
         cancel.DialogResult = DialogResult.Cancel;
         cancel.Location = new System.Drawing.Point(360, y);
         cancel.Width = 86;
         f.Controls.Add(ok);
         f.Controls.Add(cancel);
         f.AcceptButton = ok;
         f.CancelButton = cancel;
         f.ClientSize = new System.Drawing.Size(460, y + 44);

         // every element's value of the chosen parameter, worked out once per parameter
         Dictionary<string, string[]> valuesOf = new Dictionary<string, string[]>();
         string[] Values(string param)
         {
            string[]? v;
            if (!valuesOf.TryGetValue(param, out v))
            {
               v = candidates.Select(e => ValueOf(doc, e, param)).ToArray();
               valuesOf[param] = v;
            }
            return v;
         }

         bool filling = false;
         void UpdateCount()
         {
            string param = cbFilter.SelectedItem as string ?? EVERY;
            int n;
            if (param == EVERY)
               n = candidates.Count;
            else
            {
               HashSet<string> keep = new HashSet<string>(lbValues.CheckedItems.Cast<ValueItem>().Select(i => i.Value));
               n = Values(param).Count(v => keep.Contains(v));
            }
            count.Text = n + " of " + candidates.Count + " elements will be exported";
            ok.Enabled = n > 0;
         }

         void FillValues()
         {
            filling = true;
            lbValues.Items.Clear();
            string param = cbFilter.SelectedItem as string ?? EVERY;
            bool every = param == EVERY;
            lbValues.Enabled = bAll.Enabled = bNone.Enabled = !every;
            if (!every)
            {
               foreach (var g in Values(param).GroupBy(v => v).OrderBy(g => g.Key, StringComparer.CurrentCultureIgnoreCase))
                  lbValues.Items.Add(new ValueItem(g.Key, g.Count()), false);
            }
            filling = false;
            UpdateCount();
         }

         cbFilter.SelectedIndexChanged += (s, a) => FillValues();
         lbValues.ItemCheck += (s, a) =>
         {
            if (filling) return;
            // the checked state changes only after this event: count again once it has
            f.BeginInvoke(new Action(UpdateCount));
         };
         bAll.Click += (s, a) => { filling = true; for (int i = 0; i < lbValues.Items.Count; i++) lbValues.SetItemChecked(i, true); filling = false; UpdateCount(); };
         bNone.Click += (s, a) => { filling = true; for (int i = 0; i < lbValues.Items.Count; i++) lbValues.SetItemChecked(i, false); filling = false; UpdateCount(); };
         FillValues();

         if (f.ShowDialog() != DialogResult.OK)
            return null;

         Choice c = new Choice();
         string fp = cbFilter.SelectedItem as string ?? EVERY;
         c.FilterParameter = fp == EVERY ? "" : fp;
         c.FilterValues = new HashSet<string>(lbValues.CheckedItems.Cast<ValueItem>().Select(i => i.Value));
         c.NameParameter = cbName.SelectedItem as string ?? "Id";
         c.OnePerName = chkOne.Checked;
         c.Detail = cbDetail.SelectedIndex == 2 ? ViewDetailLevel.Fine : cbDetail.SelectedIndex == 1 ? ViewDetailLevel.Medium : ViewDetailLevel.Coarse;
         return c;
      }

      // One line in the list of values: the value and how many elements have it.
      private class ValueItem
      {
         public readonly string Value;
         private readonly int count;
         public ValueItem(string value, int count) { Value = value; this.count = count; }
         public override string ToString() { return (Value.Length == 0 ? "(empty)" : Value) + "   (" + count + ")"; }
      }

      // Every parameter name found on the elements and their types, after the special names.
      private static List<string> ParameterNames(Document doc, List<Element> elements)
      {
         SortedSet<string> found = new SortedSet<string>(StringComparer.CurrentCultureIgnoreCase);
         HashSet<ElementId> typesSeen = new HashSet<ElementId>();
         foreach (Element e in elements.Take(PARAMETER_SAMPLE))
         {
            foreach (Parameter p in e.Parameters)
               if (p.Definition != null && !string.IsNullOrEmpty(p.Definition.Name)) found.Add(p.Definition.Name);
            ElementId tid = e.GetTypeId();
            if (tid != ElementId.InvalidElementId && typesSeen.Add(tid))
            {
               Element? t = doc.GetElement(tid);
               if (t != null)
                  foreach (Parameter p in t.Parameters)
                     if (p.Definition != null && !string.IsNullOrEmpty(p.Definition.Name)) found.Add(p.Definition.Name);
            }
         }
         List<string> names = new List<string>(SPECIAL);
         foreach (string n in found)
            if (!names.Contains(n, StringComparer.OrdinalIgnoreCase)) names.Add(n);
         return names;
      }

      private static bool Keep(Document doc, Element e, Choice c)
      {
         if (string.IsNullOrEmpty(c.FilterParameter)) return true;
         return c.FilterValues.Contains(ValueOf(doc, e, c.FilterParameter));
      }

      // ------------------------------------------------------------------
      // Writing the OBJ
      // ------------------------------------------------------------------
      private void WriteObj(Document doc, List<Element> elements, Choice c, string file, out int objects, out int triangles)
      {
         Options opt = new Options();
         opt.DetailLevel = c.Detail;
         opt.ComputeReferences = false;
         opt.IncludeNonVisibleObjects = false;

         objects = 0;
         triangles = 0;
         int vertexBase = 1;
         HashSet<string> usedNames = new HashSet<string>();

         using (System.IO.StreamWriter w = new System.IO.StreamWriter(file, false, new UTF8Encoding(false)))
         {
            w.WriteLine("# Revit geometry from " + doc.Title);
            w.WriteLine("# units: " + (Math.Abs(SCALE - 0.3048) < 1e-9 ? "metres" : "scale " + Num(SCALE) + " x feet") + ", Z up, each solid centred on 0 0 0 (shape only)");
            w.WriteLine("# object names: " + c.NameParameter + (c.OnePerName ? " (one solid per name)" : ""));
            if (!string.IsNullOrEmpty(c.FilterParameter))
               w.WriteLine("# elements: " + c.FilterParameter + " = " + string.Join(" | ", c.FilterValues));

            foreach (Element e in elements)
            {
               string name = ValueOf(doc, e, c.NameParameter);
               if (string.IsNullOrEmpty(name)) name = e.Id.ToString();
               // a name already written stands for this element too
               if (c.OnePerName && usedNames.Contains(name)) continue;

               List<XYZ> verts = new List<XYZ>();
               List<int> tris = new List<int>();
               try
               {
                  GeometryElement? ge = e.get_Geometry(opt);
                  if (ge == null) continue;
                  Gather(ge, Transform.Identity, verts, tris);
                  if (tris.Count == 0) continue;
               }
               catch
               {
                  continue;   // one bad element must not stop the export
               }
               Centre(verts);

               // without "one solid per name", repeated names are kept apart
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
      }

      // Move the points so the middle of their bounding box is at 0, 0, 0: the shape without its place in the model.
      private static void Centre(List<XYZ> verts)
      {
         if (verts.Count == 0) return;
         double x0 = double.MaxValue, y0 = double.MaxValue, z0 = double.MaxValue;
         double x1 = double.MinValue, y1 = double.MinValue, z1 = double.MinValue;
         foreach (XYZ p in verts)
         {
            x0 = Math.Min(x0, p.X); y0 = Math.Min(y0, p.Y); z0 = Math.Min(z0, p.Z);
            x1 = Math.Max(x1, p.X); y1 = Math.Max(y1, p.Y); z1 = Math.Max(z1, p.Z);
         }
         double cx = (x0 + x1) / 2, cy = (y0 + y1) / 2, cz = (z0 + z1) / 2;
         for (int i = 0; i < verts.Count; i++)
         {
            XYZ p = verts[i];
            verts[i] = new XYZ(Math.Round(p.X - cx, DECIMALS), Math.Round(p.Y - cy, DECIMALS), Math.Round(p.Z - cz, DECIMALS));
         }
      }

      // ------------------------------------------------------------------
      // Which elements can be exported at all (the window then narrows them down)
      // ------------------------------------------------------------------
      private List<Element> Candidates(UIDocument uidoc)
      {
         Document doc = uidoc.Document;
         List<Element> picked = new List<Element>();
         ICollection<ElementId> sel = uidoc.Selection.GetElementIds();
         IEnumerable<Element> source;

         if (USE_SELECTION && sel.Count > 0)
         {
            List<Element> fromSel = new List<Element>();
            foreach (ElementId id in sel)
            {
               Element? el = doc.GetElement(id);
               if (el != null) fromSel.Add(el);
            }
            source = fromSel;
         }
         else if (ONLY_ACTIVE_VIEW)
            source = new FilteredElementCollector(doc, doc.ActiveView.Id).WhereElementIsNotElementType().ToElements();
         else
            source = new FilteredElementCollector(doc).WhereElementIsNotElementType().ToElements();

         foreach (Element e in source)
         {
            if (e == null || e.Category == null || e.ViewSpecific) continue;
            if (e.Category.CategoryType != CategoryType.Model) continue;
            if (e is RevitLinkInstance || e is ImportInstance) continue;
            picked.Add(e);
         }
         return picked;
      }

      // A parameter's value as text (looked up on the element, then on its type).
      private static string ValueOf(Document doc, Element e, string name)
      {
         if (string.IsNullOrEmpty(name)) return "";
         ElementType? type = doc.GetElement(e.GetTypeId()) as ElementType;

         switch (name.ToLowerInvariant())
         {
            case "id": return e.Id.ToString() ?? "";
            case "uniqueid": return e.UniqueId;
            case "category": return e.Category != null ? e.Category.Name : "";
            case "family": return type != null ? type.FamilyName : "";
            case "type": return type != null ? type.Name : "";
            case "family : type": return type != null ? type.FamilyName + " : " + type.Name : "";
            case "workset":
               if (!doc.IsWorkshared) return "";
               Workset? ws = doc.GetWorksetTable().GetWorkset(e.WorksetId);
               return ws != null ? ws.Name : "";
            case "level":
               Element? lvl = doc.GetElement(e.LevelId);
               if (lvl != null) return lvl.Name;
               break;   // some families keep their level in a parameter called "Level"
         }

         Parameter? p = e.LookupParameter(name);
         if ((p == null || !p.HasValue) && type != null) p = type.LookupParameter(name);
         if (p == null || !p.HasValue) return "";

         switch (p.StorageType)
         {
            case StorageType.String:
               return p.AsString() ?? "";
            case StorageType.ElementId:
               Element? other = doc.GetElement(p.AsElementId());
               return other != null ? other.Name : p.AsElementId().ToString() ?? "";
            default:
               string? shown = p.AsValueString();
               return shown ?? (p.StorageType == StorageType.Integer
                  ? p.AsInteger().ToString(CultureInfo.InvariantCulture)
                  : Num(p.AsDouble()));
         }
      }

      // ------------------------------------------------------------------
      // Geometry
      // ------------------------------------------------------------------
      // Every solid and mesh in ge (nested families too) as triangles, through tf, scaled,
      // with shared corners written once.
      private static void Gather(GeometryElement ge, Transform tf, List<XYZ> verts, List<int> tris)
      {
         Dictionary<string, int> index = new Dictionary<string, int>();
         GatherInto(ge, tf, verts, tris, index);
      }

      private static void GatherInto(GeometryElement ge, Transform tf, List<XYZ> verts, List<int> tris, Dictionary<string, int> index)
      {
         foreach (GeometryObject o in ge)
         {
            Solid? solid = o as Solid;
            if (solid != null)
            {
               if (solid.Faces.Size == 0 || solid.Volume < 1e-9) continue;   // empty or helper solids
               foreach (Face face in solid.Faces)
               {
                  Mesh? m = face.Triangulate(TRIANGULATION);
                  if (m != null) AddMesh(m, tf, verts, tris, index);
               }
               continue;
            }

            Mesh? mesh = o as Mesh;
            if (mesh != null)
            {
               AddMesh(mesh, tf, verts, tris, index);
               continue;
            }

            GeometryInstance? inst = o as GeometryInstance;
            if (inst != null)
               GatherInto(inst.GetInstanceGeometry(), tf, verts, tris, index);
         }
      }

      private static void AddMesh(Mesh m, Transform tf, List<XYZ> verts, List<int> tris, Dictionary<string, int> index)
      {
         for (int i = 0; i < m.NumTriangles; i++)
         {
            MeshTriangle t = m.get_Triangle(i);
            int a = Vertex(t.get_Vertex(0), tf, verts, index);
            int b = Vertex(t.get_Vertex(1), tf, verts, index);
            int c = Vertex(t.get_Vertex(2), tf, verts, index);
            if (a == b || b == c || a == c) continue;   // collapsed by rounding
            tris.Add(a);
            tris.Add(b);
            tris.Add(c);
         }
      }

      private static int Vertex(XYZ p, Transform tf, List<XYZ> verts, Dictionary<string, int> index)
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

      private static string Num(double d)
      {
         return d.ToString("0.####", CultureInfo.InvariantCulture);
      }

      private static string Safe(string s)
      {
         foreach (char c in System.IO.Path.GetInvalidFileNameChars()) s = s.Replace(c, '_');
         return s;
      }
   }
}
