use byteorder::{BigEndian, ByteOrder, LittleEndian};

pub enum USRPPacket {
    Start(StartPacket),
    Audio(AudioPacket),
    End(EndPacket),
    Unknown(Vec<u8>),
}

impl USRPPacket {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.len() < 32 {
            return USRPPacket::Unknown(bytes.to_vec());
        }
        let packet_type = BigEndian::read_u32(&bytes[20..24]);
        match packet_type {
            StartPacket::PACKET_TYPE => USRPPacket::Start(StartPacket {
                sequence_number: BigEndian::read_u32(&bytes[4..8]),
            }),
            AudioPacket::PACKET_TYPE => {
                let sequence_number = BigEndian::read_u32(&bytes[4..8]);
                let transmit = BigEndian::read_u32(&bytes[12..16]) == 1;
                let audio_u8 = &bytes[32..];
                let mut audio = vec![0; audio_u8.len() / 2];
                LittleEndian::read_i16_into(audio_u8, audio.as_mut_slice());
                if audio.len() > 0 {
                    USRPPacket::Audio(AudioPacket {
                        sequence_number,
                        transmit,
                        audio,
                    })
                } else {
                    USRPPacket::End(EndPacket { sequence_number })
                }
            }
            _ => USRPPacket::Unknown(bytes.to_vec()),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            USRPPacket::Start(packet) => packet.to_bytes(),
            USRPPacket::Audio(packet) => packet.to_bytes(),
            USRPPacket::End(packet) => packet.to_bytes(),
            USRPPacket::Unknown(bytes) => bytes.clone(),
        }
    }
}

pub trait USRPPacketSerialize {
    const PACKET_TYPE: u32;
    fn to_bytes(&self) -> Vec<u8>;
}

pub struct StartPacket {
    pub sequence_number: u32,
}

impl USRPPacketSerialize for StartPacket {
    const PACKET_TYPE: u32 = 2;
    fn to_bytes(&self) -> Vec<u8> {
        let mut buffer = [0; 352];
        buffer[..4].copy_from_slice(b"USRP");
        BigEndian::write_u32(&mut buffer[4..8], self.sequence_number);
        LittleEndian::write_u32(&mut buffer[20..24], Self::PACKET_TYPE);
        buffer[32..53].copy_from_slice(&[
            0x08, 0x14, 0x1F, 0xC2, 0x39, 0x0C, 0x67, 0xDE, 0x45, 0x00, 0x00, 0x07, 0x02, 0x00,
            0x32, 0x30, 0x38, 0x31, 0x33, 0x33, 0x37,
        ]);
        Vec::from(buffer)
    }
}

// End packet is just a 32 bytes packet with the audio data set to 0
pub struct AudioPacket {
    pub sequence_number: u32,
    pub transmit: bool,
    pub audio: Vec<i16>,
}

impl USRPPacketSerialize for AudioPacket {
    const PACKET_TYPE: u32 = 0;
    fn to_bytes(&self) -> Vec<u8> {
        let mut buffer = [0; 32];
        buffer[..4].copy_from_slice(b"USRP");
        BigEndian::write_u32(&mut buffer[4..8], self.sequence_number);
        BigEndian::write_u32(&mut buffer[8..12], 2);
        BigEndian::write_u32(&mut buffer[12..16], self.transmit as u32);
        BigEndian::write_u32(&mut buffer[16..20], 7);
        LittleEndian::write_u32(&mut buffer[20..24], Self::PACKET_TYPE);
        BigEndian::write_u32(&mut buffer[24..28], 0);

        let mut audio_frames = vec![0; self.audio.len() * 2];
        LittleEndian::write_i16_into(self.audio.as_slice(), audio_frames.as_mut_slice());

        let mut ret = Vec::from(buffer);
        ret.extend(audio_frames);
        ret
    }
}

pub struct EndPacket {
    pub sequence_number: u32,
}

impl USRPPacketSerialize for EndPacket {
    const PACKET_TYPE: u32 = 0;
    fn to_bytes(&self) -> Vec<u8> {
        return AudioPacket {
            sequence_number: self.sequence_number,
            transmit: false,
            audio: Vec::new(),
        }
        .to_bytes();
    }
}
