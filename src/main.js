import {invoke} from "@tauri-apps/api/core";
import {listen} from "@tauri-apps/api/event";
import "./style.css";
const $ = (id) => document.getElementById(id);
$("app").innerHTML = `<main><h1>Mosaic</h1><form id="studio">
<label>Gerät<select id="device"><option value="arc">Intel Core Ultra 7 · Arc (Windows)</option><option value="metal">Apple M5 · Metal (macOS)</option></select></label>
<label>Modell hinzufügen<input id="source" placeholder="Hugging-Face-ID oder Link zu einer .safetensors-Datei"></label>
<button type="button" id="load">Modell herunterladen</button>
<label>Installiertes Modell<select id="models"></select></label>
<label>Bildbeschreibung<textarea required maxlength="2000" id="prompt" rows="4" placeholder="Ein gelber Stadtbus auf einer regennassen Berliner Straße …"></textarea></label>
<button id="create">Bild erstellen</button></form><p role="status" id="status"></p>
<figure hidden id="result"><img id="picture" alt="Generiertes Bild"><figcaption id="path"></figcaption></figure></main>`;
let busy=false, imageUrl;
function controls(value){
 busy=value;
 for(const item of $("studio").elements)item.disabled=value;
 $("create").disabled=value || !$("models").value;
}
async function list(selected){
 const names=await invoke("models");
 $("models").replaceChildren(...names.map(n=>new Option(n.replace(/^[0-9a-f]{16}-/,""),n)));
 if(selected)$("models").value=selected;
}
async function work(message,fn){
 if(busy)return;
 controls(true);$("status").textContent=message;
 try{await fn();}catch(e){$("status").textContent=String(e);}
 finally{controls(false);}
}
$("load").onclick=()=>work("Download wird vorbereitet …",async()=>{
 const name=await invoke("download",{source:$("source").value});
 await list(name);$("status").textContent="Modell bereit.";
});
$("studio").onsubmit=e=>{
 e.preventDefault();
 work("Bild wird lokal berechnet …",async()=>{
 const path=await invoke("generate",{device:$("device").value,model:$("models").value,prompt:$("prompt").value});
 const data=await invoke("preview",{name:path});
 if(imageUrl)URL.revokeObjectURL(imageUrl);
 imageUrl=URL.createObjectURL(new Blob([data],{type:"image/png"}));
 $("picture").src=imageUrl;$("path").textContent=path;$("result").hidden=false;$("status").textContent="Bild gespeichert.";
 });
};
work("Modelle werden geladen …",async()=>{
 await listen("progress",({payload:[done,total]})=>{$("status").textContent=total?`Download · ${Math.floor(done/total*100)} %`:`Download · ${Math.floor(done/1048576)} MB`;});
 $("device").value=await invoke("profile");await list();
 $("status").textContent=$("models").value?"Bereit.":"Für den Einstieg einen vollständigen SD-1.5- oder SDXL-Checkpoint laden.";
});
