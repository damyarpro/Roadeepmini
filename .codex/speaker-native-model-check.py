import ctypes as c, os, pathlib, wave, struct, math, json, time
root=pathlib.Path(r'D:\Roadeep\Roadeep-mini\.codex\local-ai-assets')
dll_dir=root/'speaker-engine/sherpa-onnx-v1.13.8-win-x64-shared-MT-Release-no-tts/lib'
os.add_dll_directory(str(dll_dir))
lib=c.CDLL(str(dll_dir/'sherpa-onnx-c-api.dll'))
class Config(c.Structure): _fields_=[('model',c.c_char_p),('num_threads',c.c_int32),('debug',c.c_int32),('provider',c.c_char_p)]
def bind(name,args,res):
 f=getattr(lib,name); f.argtypes=args; f.restype=res; return f
create=bind('SherpaOnnxCreateSpeakerEmbeddingExtractor',[c.POINTER(Config)],c.c_void_p)
destroy=bind('SherpaOnnxDestroySpeakerEmbeddingExtractor',[c.c_void_p],None)
dimfn=bind('SherpaOnnxSpeakerEmbeddingExtractorDim',[c.c_void_p],c.c_int32)
streamfn=bind('SherpaOnnxSpeakerEmbeddingExtractorCreateStream',[c.c_void_p],c.c_void_p)
accept=bind('SherpaOnnxOnlineStreamAcceptWaveform',[c.c_void_p,c.c_int32,c.POINTER(c.c_float),c.c_int32],None)
finish=bind('SherpaOnnxOnlineStreamInputFinished',[c.c_void_p],None)
ready=bind('SherpaOnnxSpeakerEmbeddingExtractorIsReady',[c.c_void_p,c.c_void_p],c.c_int32)
compute=bind('SherpaOnnxSpeakerEmbeddingExtractorComputeEmbedding',[c.c_void_p,c.c_void_p],c.POINTER(c.c_float))
freev=bind('SherpaOnnxSpeakerEmbeddingExtractorDestroyEmbedding',[c.POINTER(c.c_float)],None)
frees=bind('SherpaOnnxDestroyOnlineStream',[c.c_void_p],None)
start=time.monotonic(); ex=create(Config(str(root/'speaker-model.onnx').encode(),2,0,b'cpu')); assert ex
try:
 dim=dimfn(ex); vectors={}; durations={}
 for name in ['spk1_snt1','spk1_snt2','spk1_snt3','spk1_snt4','spk2_snt1','spk2_snt2']:
  with wave.open(str(root/'speaker-samples'/f'{name}.wav'),'rb') as w:
   assert w.getnchannels()==1 and w.getsampwidth()==2 and w.getframerate()==16000
   frames=w.readframes(w.getnframes()); vals=struct.unpack('<'+'h'*(len(frames)//2),frames)
  durations[name]=len(vals)/16000; floats=(c.c_float*len(vals))(*(v/32768 for v in vals)); stream=streamfn(ex)
  try:
   accept(stream,16000,floats,len(vals)); finish(stream); assert ready(ex,stream)
   vp=compute(ex,stream); assert vp
   try: vec=[vp[i] for i in range(dim)]; assert all(math.isfinite(v) for v in vec); vectors[name]=vec
   finally: freev(vp)
  finally: frees(stream)
 def cosine(a,b):return sum(x*y for x,y in zip(a,b))/math.sqrt(sum(x*x for x in a)*sum(y*y for y in b))
 enroll=[sum(vectors[n][i] for n in ['spk1_snt1','spk1_snt2','spk1_snt3'])/3 for i in range(dim)]
 scores={n:round(cosine(enroll,v),4) for n,v in vectors.items()}
 print(json.dumps({'dimension':dim,'durations':durations,'scores':scores,'seconds':round(time.monotonic()-start,3)}))
finally: destroy(ex)
