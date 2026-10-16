// Prints the size of each CLAP struct RUSE mirrors, and the offset of the fields that matter,
// straight from the real header. tests/abi.rs holds the same numbers.
//
//   cc -I <clap>/include layout.c -o layout && ./layout
#include <stddef.h>
#include <stdio.h>
#include <clap/clap.h>

#define S(t) printf("size %s %zu\n", #t, sizeof(t))
#define O(t, f) printf("offset %s.%s %zu\n", #t, #f, offsetof(t, f))

int main(void) {
   S(clap_version_t);
   S(clap_plugin_entry_t);
   O(clap_plugin_entry_t, init);
   O(clap_plugin_entry_t, deinit);
   O(clap_plugin_entry_t, get_factory);
   S(clap_plugin_factory_t);
   O(clap_plugin_factory_t, create_plugin);
   S(clap_plugin_descriptor_t);
   O(clap_plugin_descriptor_t, id);
   O(clap_plugin_descriptor_t, description);
   O(clap_plugin_descriptor_t, features);
   S(clap_host_t);
   O(clap_host_t, host_data);
   O(clap_host_t, get_extension);
   O(clap_host_t, request_callback);
   S(clap_plugin_t);
   O(clap_plugin_t, plugin_data);
   O(clap_plugin_t, activate);
   O(clap_plugin_t, process);
   O(clap_plugin_t, on_main_thread);
   S(clap_process_t);
   O(clap_process_t, frames_count);
   O(clap_process_t, transport);
   O(clap_process_t, audio_outputs);
   O(clap_process_t, audio_inputs_count);
   O(clap_process_t, in_events);
   O(clap_process_t, out_events);
   S(clap_audio_buffer_t);
   O(clap_audio_buffer_t, channel_count);
   O(clap_audio_buffer_t, constant_mask);
   S(clap_event_header_t);
   O(clap_event_header_t, space_id);
   O(clap_event_header_t, flags);
   S(clap_event_param_value_t);
   O(clap_event_param_value_t, param_id);
   O(clap_event_param_value_t, cookie);
   O(clap_event_param_value_t, note_id);
   O(clap_event_param_value_t, channel);
   O(clap_event_param_value_t, value);
   S(clap_event_transport_t);
   O(clap_event_transport_t, flags);
   O(clap_event_transport_t, song_pos_seconds);
   O(clap_event_transport_t, tempo);
   O(clap_event_transport_t, bar_start);
   O(clap_event_transport_t, bar_number);
   O(clap_event_transport_t, tsig_num);
   S(clap_input_events_t);
   O(clap_input_events_t, get);
   S(clap_output_events_t);
   O(clap_output_events_t, try_push);
   S(clap_audio_port_info_t);
   O(clap_audio_port_info_t, flags);
   O(clap_audio_port_info_t, port_type);
   O(clap_audio_port_info_t, in_place_pair);
   S(clap_audio_port_configuration_request_t);
   O(clap_audio_port_configuration_request_t, channel_count);
   O(clap_audio_port_configuration_request_t, port_type);
   S(clap_plugin_audio_ports_t);
   S(clap_plugin_configurable_audio_ports_t);
   S(clap_param_info_t);
   O(clap_param_info_t, cookie);
   O(clap_param_info_t, name);
   O(clap_param_info_t, module);
   O(clap_param_info_t, min_value);
   O(clap_param_info_t, default_value);
   S(clap_plugin_params_t);
   O(clap_plugin_params_t, flush);
   S(clap_host_params_t);
   S(clap_istream_t);
   O(clap_istream_t, read);
   S(clap_ostream_t);
   O(clap_ostream_t, write);
   S(clap_plugin_state_t);
   S(clap_plugin_latency_t);
   S(clap_plugin_tail_t);
   S(clap_host_latency_t);
   return 0;
}
