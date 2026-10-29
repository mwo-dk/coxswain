//! The regional variants made from another language by rules: Canadian French from French,
//! Chilean Spanish from Argentinian, and the Spanish of the other tú countries from Chilean.
//! Each override file must be exactly what its rules make of its source today, so a text
//! changed or removed in the source never lives on, stale, in a variant.
//!
//! After a change to fr.json or es-AR.json (or to the rules), write the files again with
//! `COXSWAIN_WRITE_VARIANTS=1 cargo test -p coxswain-core --test locale_variants`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;

type Map = BTreeMap<String, Value>;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("locales")
}

fn load(code: &str) -> Map {
    let text = std::fs::read_to_string(dir().join(format!("{code}.json"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// The texts `rule` changes, each changed (every plural form of a counted text).
fn derive(source: &Map, rule: impl Fn(&str, &str) -> String) -> Map {
    let mut out = Map::new();
    for (key, v) in source {
        let new = match v {
            Value::String(s) => Value::String(rule(key, s)),
            Value::Object(o) => Value::Object(
                o.iter()
                    .map(|(f, x)| (f.clone(), Value::String(rule(key, x.as_str().unwrap()))))
                    .collect(),
            ),
            _ => continue,
        };
        if &new != v {
            out.insert(key.clone(), new);
        }
    }
    out
}

/// Quebec typography: no space before ; ! ? (the one before : stays). Only before real
/// punctuation, which ends a word: not syntax such as "% !TEX", ": !," or "· ? asks", nor
/// "!name".
fn quebec_typography(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let space = |ch: char| matches!(ch, ' ' | '\u{a0}' | '\u{202f}');
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < c.len() {
        if space(c[i]) {
            let mut j = i;
            while j < c.len() && space(c[j]) {
                j += 1;
            }
            let before = i.checked_sub(1).map(|p| c[p]);
            let mark = c.get(j).copied();
            let after = c.get(j + 1).copied();
            let drop = before.is_some_and(|b| !b.is_whitespace() && !":%·".contains(b))
                && mark.is_some_and(|m| ";!?".contains(m))
                && after.is_none_or(|a| a.is_whitespace() || "»\"’)].,".contains(a));
            if !drop {
                out.extend(&c[i..j]);
            }
            i = j;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

/// The OQLF's words for English loanwords in fr.json, as whole phrases (the config key in
/// `cloud = "all"` stays).
const QUEBEC_TERMS: &[(&str, &str)] = &[
    ("optimisé pour le cloud", "optimisé pour l’infonuagique"),
    ("Cloud : {name}", "Nuage : {name}"),
    ("entrée Cloud :", "entrée Nuage :"),
    ("dossiers cloud", "dossiers infonuagiques"),
    ("montage cloud", "montage infonuagique"),
    ("comptes cloud", "comptes infonuagiques"),
    ("services cloud", "services infonuagiques"),
    ("Dossiers du cloud", "Dossiers infonuagiques"),
    ("dans le cloud", "dans le nuage"),
    ("mon cloud", "mon nuage"),
    ("mes clouds", "mes nuages"),
    ("clouds, favoris", "nuages, favoris"),
    ("un e-mail", "un courriel"),
    ("e-mails", "courriels"),
    ("le mail", "le courriel"),
    ("d’un mail", "d’un courriel"),
    (" mails", " courriels"),
    ("modèle de chat", "modèle de conversation"),
];

fn canadian_french(key: &str, s: &str) -> String {
    if matches!(key, "feature.clouds.title" | "sidebar.clouds") && s == "Clouds" {
        return "Nuages".into();
    }
    QUEBEC_TERMS
        .iter()
        .fold(quebec_typography(s), |t, (a, b)| t.replace(a, b))
}

/// Voseo, as one word, and its tú form; with Chile's words for ticking a box and pressing a
/// key, where Argentina says tildar and apretar.
const TU: &str = "abrí=abre acercá=acerca acotá=acota activá=activa actualizá=actualiza agrandá=agranda agregá=agrega
analizá=analiza andá=ve apretá=presiona arrastrá=arrastra ayudá=ayuda bajá=baja borrá=borra buscá=busca cambiá=cambia
cerrá=cierra compará=compara comprobá=comprueba configurá=configura convertí=convierte copiá=copia corregí=corrige
cortá=corta creá=crea dejá=deja descargá=descarga deshacé=deshaz destildá=desmarca ejecutá=ejecuta elegí=elige
empaquetá=empaqueta empezá=empieza encontrá=encuentra entrá=entra escribí=escribe esperá=espera exportá=exporta
extraé=extrae guardá=guarda hacé=haz indicá=indica ingresá=ingresa instalá=instala jugá=juega leé=lee llamá=llama
mantené=mantén marcá=marca mirá=mira mostrá=muestra mové=mueve ocultá=oculta ordená=ordena pasá=pasa pegá=pega poné=pon
preguntá=pregunta presioná=presiona previsualizá=previsualiza probá=prueba recorré=recorre reiniciá=reinicia
renombrá=renombra respondé=responde revisá=revisa salí=sal seguí=sigue subí=sube terminá=termina tildá=marca tocá=toca
usá=usa volvé=vuelve
abrís=abres activás=activas apretás=presionas cambiás=cambias cerrás=cierras copiás=copias decís=dices elegís=eliges
escribís=escribes hacés=haces mirás=miras movés=mueves olvidás=olvidas pausás=pausas pedís=pides podés=puedes
preferís=prefieres querés=quieres restaurás=restauras salís=sales tenés=tienes tocás=tocas volvés=vuelves
abrilo=ábrelo achicalo=achícalo activala=actívala activalo=actívalo agregala=agrégala agregalo=agrégalo
apretalo=presiónalo arrastralo=arrástralo arreglalo=arréglalo asignalas=asígnalas asignale=asígnale bajalo=bájalo
borralo=bórralo buscala=búscala cambiala=cámbiala cambiale=cámbiale cambialo=cámbialo cerrala=ciérrala
comparalo=compáralo conectalo=conéctalo convertilo=conviértelo copialo=cópialo copialos=cópialos correlo=córrelo
dejalo=déjalo descargalo=descárgalo destildalo=desmárcalo ejecutala=ejecútala ejecutalo=ejecútalo elegila=elígela
eliminalo=elimínalo eliminalos=elimínalos ensanchalo=ensánchalo escribila=escríbela etiquetalos=etiquétalos
excluilos=exclúyelos fijate=fíjate giralo=gíralo hacele=hazle hacelo=hazlo iniciala=iníciala instalalo=instálalo
instalalos=instálalos marcalos=márcalos montala=móntala mostrame=muéstrame movelos=muévelos movete=muévete
nombrala=nómbrala pegala=pégala permitilo=permítelo ponele=ponle ponelo=ponlo preguntale=pregúntale
previsualizalo=previsualízalo quitala=quítala quitale=quítale recorrelos=recórrelos reiniciala=reiníciala
reinicialo=reinícialo restauralo=restáuralo sacale=sácale sacalo=sácalo soltalos=suéltalos tildalo=márcalo vos=tú
apreté=presioné tildar=marcar destildar=desmarcar tilda=marca tildados=marcados";

/// First-person past in users' own questions ("I wrote", "I opened"): not voseo.
const NOT_VOS: &[(&str, &str)] = &[
    ("feature.configuration.q2", "escribí"),
    ("feature.glyphs_and_fonts.q2", "elegí"),
    ("feature.move_and_rename.q2", "moví"),
    ("feature.removable_disks.q2", "volví"),
    ("feature.settings_window.q1", "escribí"),
    ("feature.undo.q2", "abrí"),
];

const CHILEAN_PHRASES: &[(&str, &str)] = &[
    ("Para vos", "Para ti"),
    ("para vos", "para ti"),
    ("con vos", "contigo"),
    ("a vos", "a ti"),
    ("esta computadora", "este computador"),
    ("otra computadora", "otro computador"),
    ("una computadora", "un computador"),
    ("la computadora", "el computador"),
    ("Computadoras hogareñas", "Computadores hogareños"),
    ("computadoras", "computadores"),
    ("computadora", "computador"),
];

fn chilean(key: &str, s: &str) -> String {
    let words: std::collections::HashMap<&str, &str> = TU
        .split_whitespace()
        .filter_map(|p| p.split_once('='))
        .collect();
    let s = CHILEAN_PHRASES
        .iter()
        .fold(s.to_string(), |t, (a, b)| t.replace(a, b));
    let word = regex::Regex::new(r"[\p{L}\p{N}_]+").unwrap();
    word.replace_all(&s, |m: &regex::Captures| {
        let w = &m[0];
        let lower = w.to_lowercase();
        match words.get(lower.as_str()) {
            Some(tu) if !NOT_VOS.contains(&(key, lower.as_str())) => {
                let mut c = tu.chars();
                let first = c.next().unwrap();
                if w.starts_with(char::is_uppercase) {
                    first.to_uppercase().chain(c).collect()
                } else {
                    tu.to_string()
                }
            }
            _ => w.to_string(),
        }
    })
    .into_owned()
}

/// Chilean Spanish with computadora, as Mexico, Peru, Venezuela, Ecuador, Bolivia, Central
/// America and the Caribbean say.
fn computadora(_key: &str, s: &str) -> String {
    let phrase = regex::Regex::new(
        r"(Computadores hogareños|este computador|otro computador|computadores|computador)([a-z]?)",
    )
    .unwrap();
    phrase
        .replace_all(s, |m: &regex::Captures| {
            if !m[2].is_empty() {
                return m[0].to_string();
            }
            match &m[1] {
                "Computadores hogareños" => "Computadoras hogareñas",
                "este computador" => "esta computadora",
                "otro computador" => "otra computadora",
                "computadores" => "computadoras",
                _ => "computadora",
            }
            .to_string()
        })
        .into_owned()
}

const COMPUTADORA: &[&str] = &[
    "es-BO", "es-CR", "es-CU", "es-DO", "es-EC", "es-GT", "es-HN", "es-MX", "es-NI", "es-PA",
    "es-PE", "es-PR", "es-SV", "es-VE",
];

/// Variants with no text of their own: their own flag, their language's texts.
const EMPTY: &[&str] = &["fr-BE", "fr-CH", "it-CH", "nl-BE", "es-CO", "es-UY"];

#[test]
fn locale_variants_are_their_rules_applied_to_their_source() {
    let es_cl = derive(&load("es-AR"), chilean);
    let mut want: Vec<(&str, Map)> = vec![
        ("fr-CA", derive(&load("fr"), canadian_french)),
        ("es-CL", es_cl.clone()),
    ];
    for code in COMPUTADORA {
        want.push((code, derive(&es_cl, computadora)));
    }
    for code in EMPTY {
        want.push((code, Map::new()));
    }
    let write = std::env::var_os("COXSWAIN_WRITE_VARIANTS").is_some();
    for (code, map) in want {
        if write {
            let mut out = Vec::new();
            let fmt = serde_json::ser::PrettyFormatter::with_indent(b" ");
            let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
            serde::Serialize::serialize(&map, &mut ser).unwrap();
            out.push(b'\n');
            std::fs::write(dir().join(format!("{code}.json")), out).unwrap();
            continue;
        }
        let have = load(code);
        for key in have.keys() {
            assert!(
                map.contains_key(key),
                "{code}: {key} is not what the rules make of its source today: write the variants again (top of this file)"
            );
        }
        for (key, v) in &map {
            assert_eq!(
                have.get(key),
                Some(v),
                "{code}: {key} differs from its rules: write the variants again (top of this file)"
            );
        }
    }
}

#[test]
fn locale_variant_rules() {
    assert_eq!(
        quebec_typography("Supprimer ? Oui ; non ! % !TEX · ? demande !node"),
        "Supprimer? Oui; non! % !TEX · ? demande !node"
    );
    assert_eq!(quebec_typography("Nom : {name}"), "Nom : {name}");
    assert_eq!(
        chilean("x", "Escribí un comando y apretá Enter; tildalo si querés."),
        "Escribe un comando y presiona Enter; márcalo si quieres."
    );
    assert_eq!(chilean("feature.undo.q2", "Solo abrí el archivo."), "Solo abrí el archivo.");
    assert_eq!(chilean("x", "en esta computadora, para vos"), "en este computador, para ti");
    assert_eq!(
        computadora("x", "en este computador; Computadores hogareños"),
        "en esta computadora; Computadoras hogareñas"
    );
}
