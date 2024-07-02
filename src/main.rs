use std::env;
use std::fs;
use std::process::exit;
use std::time::Duration;

use serialport::DataBits;
use serialport::Parity;
use serialport::SerialPort;
use serialport::StopBits;

enum ExitCodes {
	Ok = 0 ,
	FileError = 1,
	FilesizeError = 2,
	DeviceError = 3,
	InitWriteError = 4,
	WriteError = 5,
	AckError = 6,
	ParametersError = 999
}

fn main() {
	let args: Vec<String> = env::args().collect();

	if args.len() != 3 {
		println!("Usage: TDH3flash-rs <device> <firmware file>");
		exit(ExitCodes::ParametersError as i32);
	}

    let device = &args[1];
	let filename = &args[2];

	pre_check_firmware(filename);
	let file_content = read_firmware(filename);
	check_firmware(filename, &file_content);
	let content_length = file_content.len();
	let padded_length = get_padded_length(&file_content);
	let port = open_port(device);

    println!("filename: {filename}.");
    println!("device: {device}.");
	println!("firmware length: {content_length} b / {padded_length} b.");

	println!("\nTurn off the radio, hold PTT and turn the radio on keeping the PTT button held.");

	upload_firmware(port,file_content.as_ref());

	exit(ExitCodes::Ok as i32);
}

fn read_firmware(filename: &String) -> Vec<u8> {
	return fs::read(filename.clone())
	    .unwrap_or_else(|error| { 
			eprintln!("Error reading file \'{filename}\'. Error: {error}");
			exit(ExitCodes::FileError as i32); 
		});
}

fn get_padded_length(file_content: &Vec<u8>) -> i32 {
	let content_length = file_content.len();
	let len: i32 = ((content_length as f64 / 32.0).ceil() as i32) * 32;
	return len;
}

fn pre_check_firmware(filename: &String){
	let is_special_file = match fs::metadata(filename){
		Ok(m) => !m.is_file(),
		Err(_e) => false
	};
	if is_special_file {
		eprintln!("\'{filename}\' is not a file."); 
		exit(ExitCodes::FileError as i32); 
	}
}

fn check_firmware(filename: &String, file_content: &Vec<u8>){
	let len = get_padded_length(file_content);
	if len < 40000 || len > 65536 { 
		eprintln!("\'{filename}\' is not the correct size to be a valid firmware file."); 
		exit(ExitCodes::FilesizeError as i32); 
	}
}

fn open_port(device: &String) -> Box<dyn SerialPort> {
	let baud_rate: u32 = 115200;

    return serialport::new(device, baud_rate)
        .stop_bits(StopBits::One)
        .data_bits(DataBits::Eight)
		.parity(Parity::None)
        .timeout(Duration::from_millis(500))
		.flow_control(serialport::FlowControl::None)
        .open()
		.unwrap_or_else(|error| {
			eprintln!("Error opening device \'{device}\'. Error: {error}"); 
			exit(ExitCodes::DeviceError as i32); 	
		});
}

fn upload_firmware(mut port: Box<dyn SerialPort>, data: &Vec<u8>){
	let mut found = false;
	print!("Waiting...");
	loop {
		let byte = read_byte_compat(port.as_mut());
		if byte == -1 {
			if found { break; }
		} else if byte == 0xa5 {
			if !found {
				found = true;
				println!("\n\nRadio found...");
				let init: Vec<u8> = vec![ 	0xA0, 0xEE, 0x74, 0x71, 0x07, 0x74, 0x55, 0x55,
											0x55, 0x55 ,0x55 ,0x55 ,0x55 ,0x55 ,0x55 ,0x55,
											0x55, 0x55 ,0x55 ,0x55 ,0x55 ,0x55 ,0x55 ,0x55,
											0x55, 0x55 ,0x55 ,0x55 ,0x55 ,0x55 ,0x55 ,0x55,
											0x55, 0x55 ,0x55 ,0x55];
				port.write_all(init.as_ref()).unwrap_or_else(|error| {
				eprintln!("Error writing init data. Error: {error}");
				exit(ExitCodes::InitWriteError as i32); 
				});
			}
		} else {
			eprintln!("Serial read unexpected data (HS)");
			exit(ExitCodes::InitWriteError as i32);
		}
		print!(".");
	}

	println!("Init OK.");

	let len = get_padded_length(data.as_ref());
	let mut padded_data: Vec<u8> = data.to_owned();
	if padded_data.len() != len as usize {
		padded_data.resize((len+32) as usize, 0);
	}

	for blk in 0..((len/32)) {
		let chunk = &padded_data[(blk*32) as usize..((blk*32)+32) as usize];
		if (&blk % 64) == 0 {
			let byte_pos = &blk*32;
			println!("Flashing {byte_pos}B");
		}
		let mut packet: Vec<u8> = vec![0;4];
		packet[0] = 0xa1;
		if (blk*32)+32 >= len {
			packet[0] += 1;
		}
		packet[1] = ((blk >> 8) & 0xff) as u8;
		packet[2] = (blk & 0xff) as u8;
		for b in chunk {
			packet[3] = packet[3].wrapping_add(*b);
		}
		packet.extend(chunk);

		port.write_all(packet.as_ref()).unwrap_or_else(|_error| {
			let byte_pos = &blk*32;
			eprintln!("Write error at {byte_pos}b.");
			exit(ExitCodes::WriteError as i32);
		});
		port.flush().unwrap_or_else(|_error| {
		 	eprintln!("Error flushing serial buffer.");
		 	exit(ExitCodes::WriteError as i32);
		});
		let mut ack: Vec<u8> = vec![0];
		port.read(ack.as_mut_slice()).unwrap_or_else(|_error| {
			let byte_pos = &blk*32;
			eprintln!("Ack read error at {byte_pos}b.");
			exit(ExitCodes::AckError as i32);
		});
	}
	println!("\nDone.");
	exit(0);
}

fn read_byte_compat(port: &mut dyn SerialPort) -> i16 {
	let mut buf: Vec<u8> = vec![0];
	return match port.read(buf.as_mut_slice()) {
		Ok(_bytes_read) => {
			return buf[0] as i16;
		},
		Err(_e) => -1
	};
}