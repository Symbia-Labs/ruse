// A small CLAP host written in C against the real header. It loads a built .clap and walks the entry,
// factory, ports, parameters, processing and state, so a mistake in RUSE's hand-written layouts shows up
// here as a failure or a crash.
//
//   cc -I <clap>/include host.c -o host && ./host "Warm Saturator.clap/Contents/MacOS/Warm Saturator"
#include <dlfcn.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <clap/clap.h>

static int failures = 0;

#define CHECK(cond, ...)                                                                           \
   do {                                                                                            \
      if (cond) {                                                                                  \
         printf("PASS ");                                                                          \
      } else {                                                                                     \
         printf("FAIL ");                                                                          \
         failures++;                                                                               \
      }                                                                                            \
      printf(__VA_ARGS__);                                                                         \
      printf("\n");                                                                                \
   } while (0)

static const void *host_get_extension(const clap_host_t *h, const char *id) {
   (void)h;
   (void)id;
   return NULL;
}
static void host_noop(const clap_host_t *h) { (void)h; }

typedef struct {
   clap_event_param_value_t ev[4];
   uint32_t                 n;
} EventList;

static uint32_t in_size(const clap_input_events_t *l) { return ((EventList *)l->ctx)->n; }
static const clap_event_header_t *in_get(const clap_input_events_t *l, uint32_t i) {
   return &((EventList *)l->ctx)->ev[i].header;
}
static bool out_push(const clap_output_events_t *l, const clap_event_header_t *e) {
   (void)l;
   (void)e;
   return true;
}

typedef struct {
   uint8_t *bytes;
   size_t   size, at;
} Memory;

static int64_t mem_write(const clap_ostream_t *s, const void *buf, uint64_t n) {
   Memory *m = (Memory *)s->ctx;
   m->bytes = realloc(m->bytes, m->size + n);
   memcpy(m->bytes + m->size, buf, n);
   m->size += n;
   return (int64_t)n;
}
static int64_t mem_read(const clap_istream_t *s, void *buf, uint64_t n) {
   Memory *m = (Memory *)s->ctx;
   uint64_t left = m->size - m->at;
   if (n > left) n = left;
   memcpy(buf, m->bytes + m->at, n);
   m->at += n;
   return (int64_t)n;
}

int main(int argc, char **argv) {
   if (argc < 2) {
      fprintf(stderr, "usage: host PATH-TO-LIBRARY\n");
      return 2;
   }
   void *lib = dlopen(argv[1], RTLD_NOW);
   CHECK(lib != NULL, "dlopen %s", lib ? "ok" : dlerror());
   if (!lib) return 1;
   const clap_plugin_entry_t *entry = dlsym(lib, "clap_entry");
   CHECK(entry != NULL, "clap_entry is exported");
   if (!entry) return 1;
   CHECK(clap_version_is_compatible(entry->clap_version), "entry version %u.%u.%u", entry->clap_version.major,
         entry->clap_version.minor, entry->clap_version.revision);
   CHECK(entry->init(argv[1]), "entry init");
   const clap_plugin_factory_t *factory = entry->get_factory(CLAP_PLUGIN_FACTORY_ID);
   CHECK(factory != NULL, "plugin factory");
   CHECK(factory->get_plugin_count(factory) >= 1, "plugin count %u", factory->get_plugin_count(factory));
   const clap_plugin_descriptor_t *d = factory->get_plugin_descriptor(factory, 0);
   CHECK(d && d->id && d->name && d->vendor && d->version && d->features, "descriptor fields present");
   printf("INFO id=%s name=%s vendor=%s version=%s features:", d->id, d->name, d->vendor, d->version);
   for (const char *const *f = d->features; *f; f++) printf(" %s", *f);
   printf("\n");

   clap_host_t host = {CLAP_VERSION, NULL, "c-host", "RUSE tests", "", "1", host_get_extension, host_noop, host_noop, host_noop};
   const clap_plugin_t *p = factory->create_plugin(factory, &host, d->id);
   CHECK(p != NULL, "create_plugin");
   CHECK(p->init(p), "plugin init");

   const clap_plugin_audio_ports_t *ports = p->get_extension(p, CLAP_EXT_AUDIO_PORTS);
   CHECK(ports && ports->count(p, true) == 1 && ports->count(p, false) == 1, "one audio port in and one out");
   clap_audio_port_info_t pi;
   memset(&pi, 0, sizeof pi);
   CHECK(ports->get(p, 0, true, &pi), "port info: id %u name %s channels %u flags %u in_place_pair %u", pi.id, pi.name,
         pi.channel_count, pi.flags, pi.in_place_pair);
   uint32_t channels = pi.channel_count;

   const clap_plugin_params_t *params = p->get_extension(p, CLAP_EXT_PARAMS);
   CHECK(params != NULL, "params extension");
   uint32_t n = params->count(p);
   printf("INFO %u parameters\n", n);
   clap_id first_writable = CLAP_INVALID_ID;
   double  max_value = 0;
   for (uint32_t i = 0; i < n; i++) {
      clap_param_info_t info;
      memset(&info, 0, sizeof info);
      CHECK(params->get_info(p, i, &info), "param %u: id %u name %s module %s range %g..%g default %g flags %#x", i, info.id,
            info.name, info.module, info.min_value, info.max_value, info.default_value, info.flags);
      double v = NAN;
      CHECK(params->get_value(p, info.id, &v) && v == info.default_value, "param %u starts at its default", i);
      char text[64];
      CHECK(params->value_to_text(p, info.id, info.default_value, text, sizeof text), "param %u text '%s'", i, text);
      if (first_writable == CLAP_INVALID_ID && !(info.flags & CLAP_PARAM_IS_READONLY)) {
         first_writable = info.id;
         max_value = info.max_value;
      }
   }

   CHECK(p->activate(p, 48000.0, 32, 512), "activate");
   CHECK(p->start_processing(p), "start_processing");

   const uint32_t frames = 256;
   float *in[64], *out[64];
   for (uint32_t c = 0; c < channels; c++) {
      in[c] = calloc(frames, sizeof(float));
      out[c] = calloc(frames, sizeof(float));
      for (uint32_t i = 0; i < frames; i++) in[c][i] = 0.25f * sinf(0.05f * (float)i);
   }
   clap_audio_buffer_t ib = {in, NULL, channels, 0, 0};
   clap_audio_buffer_t ob = {out, NULL, channels, 0, 0};
   EventList events;
   memset(&events, 0, sizeof events);
   clap_input_events_t  in_events = {&events, in_size, in_get};
   clap_output_events_t out_events = {NULL, out_push};
   clap_process_t       pr = {0, frames, NULL, &ib, &ob, 1, 1, &in_events, &out_events};
   clap_process_status  st = p->process(p, &pr);
   CHECK(st != CLAP_PROCESS_ERROR, "process status %d", st);
   int finite = 1;
   for (uint32_t c = 0; c < channels; c++)
      for (uint32_t i = 0; i < frames; i++) finite &= isfinite(out[c][i]);
   CHECK(finite, "output is finite on %u channels", channels);

   if (first_writable != CLAP_INVALID_ID) {
      events.n = 1;
      events.ev[0].header.size = sizeof(clap_event_param_value_t);
      events.ev[0].header.time = 0;
      events.ev[0].header.space_id = CLAP_CORE_EVENT_SPACE_ID;
      events.ev[0].header.type = CLAP_EVENT_PARAM_VALUE;
      events.ev[0].header.flags = 0;
      events.ev[0].param_id = first_writable;
      events.ev[0].cookie = NULL;
      events.ev[0].note_id = -1;
      events.ev[0].port_index = -1;
      events.ev[0].channel = -1;
      events.ev[0].key = -1;
      events.ev[0].value = max_value;
      pr.steady_time = frames;
      st = p->process(p, &pr);
      double v = NAN;
      CHECK(st != CLAP_PROCESS_ERROR && params->get_value(p, first_writable, &v) && v == max_value,
            "a parameter event sets param %u to %g (reads %g)", first_writable, max_value, v);
      events.n = 0;
   }

   const clap_plugin_state_t *state = p->get_extension(p, CLAP_EXT_STATE);
   CHECK(state != NULL, "state extension");
   Memory saved = {NULL, 0, 0};
   clap_ostream_t os = {&saved, mem_write};
   CHECK(state->save(p, &os), "state saved %zu bytes", saved.size);
   const clap_plugin_t *q = factory->create_plugin(factory, &host, d->id);
   CHECK(q && q->init(q), "second instance");
   clap_istream_t is = {&saved, mem_read};
   CHECK(state->load(q, &is), "state loaded into the second instance");
   if (first_writable != CLAP_INVALID_ID) {
      double a = NAN, b = NAN;
      params->get_value(p, first_writable, &a);
      params->get_value(q, first_writable, &b);
      CHECK(a == b, "restored value %g matches %g", b, a);
   }

   const clap_plugin_latency_t *latency = p->get_extension(p, CLAP_EXT_LATENCY);
   const clap_plugin_tail_t    *tail = p->get_extension(p, CLAP_EXT_TAIL);
   printf("INFO latency %u tail %u\n", latency ? latency->get(p) : 0, tail ? tail->get(p) : 0);

   p->stop_processing(p);
   p->deactivate(p);
   p->destroy(p);
   q->destroy(q);
   entry->deinit();
   dlclose(lib);
   printf("%s\n", failures ? "FAILED" : "ALL PASSED");
   return failures ? 1 : 0;
}
