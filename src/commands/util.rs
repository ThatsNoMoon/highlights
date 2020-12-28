// Copyright 2020 Benjamin Scherer
// Licensed under the Open Software License version 3.0

use serenity::{
	client::Context,
	model::{
		channel::{ChannelType, GuildChannel},
		id::{ChannelId, GuildId, UserId},
		prelude::User,
	},
};

use crate::{regex, Error};
use std::{collections::HashMap, iter::FromIterator};

#[macro_export]
macro_rules! require_guild {
	($ctx:expr, $message:expr) => {{
		match $message.guild_id {
			None => {
				return $crate::util::error(
					$ctx,
					$message,
					"You must run this command in a server!",
				)
				.await
				}
			Some(id) => id,
			}
		}};
}

#[macro_export]
macro_rules! require_nonempty_args {
	($args:expr, $ctx:expr, $message:expr) => {{
		if $args == "" {
			return $crate::util::question($ctx, $message).await;
			}
		}};
}

#[macro_export]
macro_rules! require_empty_args {
	($args:expr, $ctx:expr, $message:expr) => {{
		if $args != "" {
			return $crate::util::question($ctx, $message).await;
			}
		}};
}

#[derive(Debug, Default)]
pub struct UsersFromArgs<'args> {
	pub found: Vec<User>,
	pub not_found: Vec<u64>,
	pub invalid: Vec<&'args str>,
}

pub async fn get_users_from_args<'args>(
	ctx: &Context,
	args: &'args str,
) -> UsersFromArgs<'args> {
	let mut results = UsersFromArgs::default();

	for word in args.split_whitespace() {
		match regex!(r"([0-9]{16,20})|<@!?([0-9]{16,20})>").captures(word) {
			Some(captures) => {
				let id = captures
					.get(1)
					.or_else(|| captures.get(2))
					.unwrap()
					.as_str()
					.parse()
					.unwrap();

				match ctx.http.get_user(id).await {
					Ok(user) => results.found.push(user),
					Err(_) => results.not_found.push(id),
				}
			}
			None => results.invalid.push(word),
		}
	}

	results
}

pub async fn get_text_channels_in_guild(
	ctx: &Context,
	guild_id: GuildId,
) -> Result<HashMap<ChannelId, GuildChannel>, Error> {
	let channels = ctx
		.cache
		.guild_channels(guild_id)
		.await
		.ok_or("Couldn't get guild to get channels")?;
	let channels = channels
		.into_iter()
		.filter(|(_, channel)| channel.kind == ChannelType::Text)
		.collect();

	Ok(channels)
}

pub async fn get_readable_channels_from_args<'args, 'c>(
	ctx: &Context,
	author_id: UserId,
	channels: &'c HashMap<ChannelId, GuildChannel>,
	args: &'args str,
) -> Result<ReadableChannelsFromArgs<'args, 'c>, Error> {
	let all_channels = get_channels_from_args(channels, args);

	let mut result = ReadableChannelsFromArgs::default();

	result.not_found = all_channels.not_found;

	for (channel, arg) in all_channels.found {
		let user_can_read =
			crate::util::user_can_read_channel(ctx, channel, author_id).await?;

		let self_can_read = crate::util::user_can_read_channel(
			ctx,
			channel,
			ctx.cache.current_user_id().await,
		)
		.await?;

		if !user_can_read {
			result.user_cant_read.push((channel, arg));
		} else if !self_can_read {
			result.self_cant_read.push(channel);
		} else {
			result.found.push(channel);
		}
	}

	Ok(result)
}

pub fn get_ids_from_args(args: &str) -> Vec<Result<(ChannelId, &str), &str>> {
	args.split_whitespace()
		.map(|arg| arg.parse().map(|id| (ChannelId(id), arg)).map_err(|_| arg))
		.collect()
}

fn get_channels_from_args<'args, 'c>(
	channels: &'c HashMap<ChannelId, GuildChannel>,
	args: &'args str,
) -> ChannelsFromArgs<'args, 'c> {
	args.split_whitespace()
		.map(|arg| get_channel_from_arg(channels, arg))
		.collect()
}

fn get_channel_from_arg<'arg, 'c>(
	channels: &'c HashMap<ChannelId, GuildChannel>,
	arg: &'arg str,
) -> Result<(&'c GuildChannel, &'arg str), &'arg str> {
	if let Ok(id) = arg.parse::<u64>() {
		return match channels.get(&ChannelId(id)) {
			Some(c) => Ok((c, arg)),
			None => Err(arg),
		};
	}

	if let Some(id) = arg
		.strip_prefix("<#")
		.and_then(|arg| arg.strip_suffix(">"))
		.and_then(|arg| arg.parse::<u64>().ok())
	{
		return match channels.get(&ChannelId(id)) {
			Some(c) => Ok((c, arg)),
			None => Err(arg),
		};
	}

	let mut iter = channels
		.iter()
		.map(|(_, channel)| channel)
		.filter(|channel| channel.name.as_str().eq_ignore_ascii_case(arg));

	if let Some(first) = iter.next() {
		if iter.next().is_none() {
			return Ok((first, arg));
		}
	}

	Err(arg)
}

#[derive(Debug, Default)]
struct ChannelsFromArgs<'args, 'c> {
	not_found: Vec<&'args str>,
	found: Vec<(&'c GuildChannel, &'args str)>,
}

impl<'args, 'c> FromIterator<Result<(&'c GuildChannel, &'args str), &'args str>>
	for ChannelsFromArgs<'args, 'c>
{
	fn from_iter<
		T: IntoIterator<Item = Result<(&'c GuildChannel, &'args str), &'args str>>,
	>(
		iter: T,
	) -> Self {
		let mut result = Self::default();
		iter.into_iter().for_each(|res| match res {
			Ok(c) => result.found.push(c),
			Err(arg) => result.not_found.push(arg),
		});
		result
	}
}

#[derive(Debug, Default)]
pub struct ReadableChannelsFromArgs<'args, 'c> {
	pub not_found: Vec<&'args str>,
	pub found: Vec<&'c GuildChannel>,
	pub user_cant_read: Vec<(&'c GuildChannel, &'args str)>,
	pub self_cant_read: Vec<&'c GuildChannel>,
}
