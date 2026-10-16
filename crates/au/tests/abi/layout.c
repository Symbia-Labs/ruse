// Prints the size of each Audio Unit struct RUSE mirrors, the offset of the fields that matter, and the
// value of each constant it uses, straight from the SDK headers. tests/abi.rs holds the same numbers.
//
//   cc layout.c -framework AudioToolbox -framework CoreFoundation -o layout && ./layout
#include <AudioToolbox/AudioToolbox.h>
#include <stddef.h>
#include <stdio.h>

#define S(t) printf("size %s %zu\n", #t, sizeof(t))
#define O(t, f) printf("offset %s.%s %zu\n", #t, #f, offsetof(t, f))
#define C(n) printf("const %s %lld\n", #n, (long long)(n))

int main(void) {
   S(AudioComponentDescription);
   O(AudioComponentDescription, componentManufacturer);
   O(AudioComponentDescription, componentFlagsMask);
   S(AudioComponentPlugInInterface);
   O(AudioComponentPlugInInterface, Close);
   O(AudioComponentPlugInInterface, Lookup);
   O(AudioComponentPlugInInterface, reserved);
   S(AudioStreamBasicDescription);
   O(AudioStreamBasicDescription, mFormatID);
   O(AudioStreamBasicDescription, mBytesPerFrame);
   O(AudioStreamBasicDescription, mChannelsPerFrame);
   O(AudioStreamBasicDescription, mReserved);
   S(AudioBuffer);
   O(AudioBuffer, mDataByteSize);
   O(AudioBuffer, mData);
   S(AudioBufferList);
   O(AudioBufferList, mBuffers);
   S(AudioTimeStamp);
   O(AudioTimeStamp, mHostTime);
   O(AudioTimeStamp, mRateScalar);
   O(AudioTimeStamp, mSMPTETime);
   O(AudioTimeStamp, mFlags);
   S(AudioUnitParameterInfo);
   O(AudioUnitParameterInfo, unitName);
   O(AudioUnitParameterInfo, clumpID);
   O(AudioUnitParameterInfo, cfNameString);
   O(AudioUnitParameterInfo, unit);
   O(AudioUnitParameterInfo, minValue);
   O(AudioUnitParameterInfo, defaultValue);
   O(AudioUnitParameterInfo, flags);
   S(AUChannelInfo);
   O(AUChannelInfo, outChannels);
   S(AudioUnitConnection);
   O(AudioUnitConnection, sourceOutputNumber);
   O(AudioUnitConnection, destInputNumber);
   S(AURenderCallbackStruct);
   O(AURenderCallbackStruct, inputProcRefCon);
   S(HostCallbackInfo);
   O(HostCallbackInfo, beatAndTempoProc);
   O(HostCallbackInfo, transportStateProc);
   O(HostCallbackInfo, transportStateProc2);
   S(AudioUnitParameterEvent);
   O(AudioUnitParameterEvent, eventType);
   O(AudioUnitParameterEvent, eventValues);
   S(AudioUnitParameterStringFromValue);
   O(AudioUnitParameterStringFromValue, outString);
   S(AudioUnitParameterValueFromString);
   O(AudioUnitParameterValueFromString, outValue);
   S(AudioUnitParameterNameInfo);
   O(AudioUnitParameterNameInfo, outName);
   S(AUPreset);

   C(kAudioUnitInitializeSelect);
   C(kAudioUnitUninitializeSelect);
   C(kAudioUnitGetPropertyInfoSelect);
   C(kAudioUnitGetPropertySelect);
   C(kAudioUnitSetPropertySelect);
   C(kAudioUnitAddPropertyListenerSelect);
   C(kAudioUnitRemovePropertyListenerSelect);
   C(kAudioUnitRemovePropertyListenerWithUserDataSelect);
   C(kAudioUnitAddRenderNotifySelect);
   C(kAudioUnitRemoveRenderNotifySelect);
   C(kAudioUnitGetParameterSelect);
   C(kAudioUnitSetParameterSelect);
   C(kAudioUnitScheduleParametersSelect);
   C(kAudioUnitRenderSelect);
   C(kAudioUnitResetSelect);
   C(kAudioUnitErr_InvalidProperty);
   C(kAudioUnitErr_InvalidParameter);
   C(kAudioUnitErr_InvalidElement);
   C(kAudioUnitErr_NoConnection);
   C(kAudioUnitErr_FailedInitialization);
   C(kAudioUnitErr_TooManyFramesToProcess);
   C(kAudioUnitErr_FormatNotSupported);
   C(kAudioUnitErr_Uninitialized);
   C(kAudioUnitErr_InvalidScope);
   C(kAudioUnitErr_PropertyNotWritable);
   C(kAudioUnitErr_InvalidPropertyValue);
   C(kAudioUnitErr_Initialized);
   C(kAudioUnitScope_Global);
   C(kAudioUnitScope_Input);
   C(kAudioUnitScope_Output);
   C(kAudioUnitProperty_ClassInfo);
   C(kAudioUnitProperty_MakeConnection);
   C(kAudioUnitProperty_SampleRate);
   C(kAudioUnitProperty_ParameterList);
   C(kAudioUnitProperty_ParameterInfo);
   C(kAudioUnitProperty_StreamFormat);
   C(kAudioUnitProperty_ElementCount);
   C(kAudioUnitProperty_Latency);
   C(kAudioUnitProperty_SupportedNumChannels);
   C(kAudioUnitProperty_MaximumFramesPerSlice);
   C(kAudioUnitProperty_ParameterValueStrings);
   C(kAudioUnitProperty_TailTime);
   C(kAudioUnitProperty_BypassEffect);
   C(kAudioUnitProperty_LastRenderError);
   C(kAudioUnitProperty_SetRenderCallback);
   C(kAudioUnitProperty_HostCallbacks);
   C(kAudioUnitProperty_InPlaceProcessing);
   C(kAudioUnitProperty_ShouldAllocateBuffer);
   C(kAudioUnitProperty_ParameterStringFromValue);
   C(kAudioUnitProperty_ParameterIDName);
   C(kAudioUnitProperty_ParameterValueFromString);
   C(kAudioUnitRenderAction_PreRender);
   C(kAudioUnitRenderAction_PostRender);
   C(kAudioUnitRenderAction_OutputIsSilence);
   C(kAudioUnitParameterUnit_Generic);
   C(kAudioUnitParameterUnit_Indexed);
   C(kAudioUnitParameterUnit_Boolean);
   C(kAudioUnitParameterUnit_Percent);
   C(kAudioUnitParameterUnit_Seconds);
   C(kAudioUnitParameterUnit_Hertz);
   C(kAudioUnitParameterUnit_Decibels);
   C(kAudioUnitParameterUnit_Milliseconds);
   C(kAudioUnitParameterFlag_CFNameRelease);
   C(kAudioUnitParameterFlag_ValuesHaveStrings);
   C(kAudioUnitParameterFlag_DisplayLogarithmic);
   C(kAudioUnitParameterFlag_HasCFNameString);
   C(kAudioUnitParameterFlag_MeterReadOnly);
   C(kAudioUnitParameterFlag_IsReadable);
   C(kAudioUnitParameterFlag_IsWritable);
   C(kParameterEvent_Immediate);
   C(kParameterEvent_Ramped);
   C(kAudioFormatLinearPCM);
   C(kAudioFormatFlagIsFloat);
   C(kAudioFormatFlagIsPacked);
   C(kAudioFormatFlagIsNonInterleaved);
   C(kAudioFormatFlagsNativeFloatPacked);
   C(kAudioTimeStampSampleTimeValid);
   C(kAudioTimeStampHostTimeValid);
   C(kAudioUnitType_Effect);
   C(kCFStringEncodingUTF8);
   C(kCFNumberSInt32Type);
   return 0;
}
