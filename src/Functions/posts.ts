import { ApiResponse, ApisauceInstance, create } from "apisauce";
import { Context } from "telegraf";
import { InlineKeyboardMarkup } from "telegraf/typings/core/types/typegram";
import { CAPI } from "../Controllers";
import { ILogger, IOptions } from "../Interfaces";

interface IGetPostsParams {
    defaultTags?: string[];
    forbiddenTags?: string[];
}

async function getURL(defaultTags: string[], forbiddenTags: string[]): Promise<string> {
    if (defaultTags.length + forbiddenTags.length === 0) return '';
    let currentLength: number = defaultTags.length + forbiddenTags.length;
    while (currentLength-- > 40) forbiddenTags.pop();
    const defaultTagsStr: string = defaultTags.join('+');
    const forbittenTagsStr: string = forbiddenTags.join('+-');
    const actualTags: string[] = [];
    if (defaultTagsStr.length) actualTags.push(defaultTagsStr);
    if (forbittenTagsStr.length) actualTags.push(forbittenTagsStr);
    return `?tags=${actualTags.join('+-')}`;
}

async function getPosts(params: IGetPostsParams): Promise<ApiResponse<any>> {
    const defaultTags = [...(params.defaultTags || [])];
    const forbiddenTags = [...(params.forbiddenTags || [])];
    const apiPromise: ApisauceInstance = CAPI.GetComponent();
    const urlPromise: Promise<string> = getURL(defaultTags ?? [], forbiddenTags ?? []);
    const [api, url] = await Promise.all([apiPromise, urlPromise]);
    return api.get(url);
}

function removePostsWithTags(posts: any[], forbiddenTags: string[]) {
    const filteredPosts: any[] = [...posts];
    const returnedPosts: any[] = [];
    for (const post of filteredPosts) {
        const allTags: string[] = [];
        for (const category of Object.keys(post.tags)) {
            const actualCategory = post.tags[category].map((tag: string) => tag.toUpperCase());
            allTags.push(...actualCategory);
        }

        const hasForbiddenTag: boolean = forbiddenTags.some((tag) => allTags.includes(tag.toUpperCase()));
        if (!hasForbiddenTag) returnedPosts.push(post);
    }
    return returnedPosts;
}



function postContainsTags(post: any, tags: string[], options?: IOptions.IPostContainTagsOptions): boolean {
    const allTags: string[] = [];
    for (const category of Object.keys(post.tags)) {
        const actualCategory = post.tags[category].map((tag: string) => tag.toUpperCase());
        allTags.push(...actualCategory);
    }
    const exists = tags.some((tag) => allTags.includes(tag.toUpperCase()));
    if (options?.fatal && exists) throw { botMessage: 'Post contains forbidden tags' };
    return exists;
}

function removeDuplicatePosts(posts: any[], added: any[]): any[] {
    if (!posts.length) return [...added];
    const uniquePosts: any[] = [...posts];
    for (const post of added) {
        const isDuplicate: boolean = uniquePosts.some((uniquePost) => uniquePost.id === post.id);
        if (!isDuplicate) uniquePosts.push(post);
    }
    return uniquePosts;
}



function getPreferredSource(sources: string[]) {
    if (!sources.length) return null;
    const reg: RegExp[] = [
        /https\:\/\/www\.twitter\.com/,
        /https\:\/\/www\.furaffinity\.com/,
        /https\:\/\/www\.tumblr\.com/,
        /https\:\/\/www\.deviantart\.com/,
        /https\:\/\/www\.pixiv\.net/,
    ];

    for (const regExp of reg) {
        for (const source of sources) {
            if (regExp.test(source)) return source;
        }
    }
    return sources[0];
}


async function sendPostsToChat(ctx: Context, posts: any[], logger?: ILogger) {
    for (const post of posts) {
        const { ext, url } = post.file;
        const isAnimation: boolean = ['webm', 'gif', 'mp4'].includes(ext);
        const e621src = `https://e621.net/post/show/${post.id}`;
        const replyMarkup: InlineKeyboardMarkup = {
            inline_keyboard: [
                [
                    {
                        text: 'Send',
                        callback_data: JSON.stringify({ id: post.id, type: 'send' }),
                    },
                ],
                [
                    {
                        text: 'Check e621 Src',
                        url: e621src
                    },
                    {
                        text: 'Check src',
                        url: getPreferredSource(post.sources) || e621src
                    }
                ],
                [
                    {
                        text: 'Erase',
                        callback_data: JSON.stringify({ id: post.id, type: 'erase' })
                    }
                ]
            ]

        };
        if (isAnimation && ext === 'gif') {
            ctx.replyWithAnimation(url, { reply_markup: replyMarkup }).catch((err: any) => logger?.error(err));
        } else {
            ctx.replyWithPhoto(url, { reply_markup: replyMarkup }).catch((err: any) => logger?.error(err));
        }

    }
}


function prepareMarkdown(input: string) {
    if (!input) return '';
    const replaceable = ['_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!'];
    for (const char of replaceable) {
        // @ts-ignore
        input = input.replaceAll(char, '\\' + char);
    }
    return input;
}



export default { getPosts, removeDuplicatePosts, removePostsWithTags, sendPostsToChat, getPreferredSource, prepareMarkdown, postContainsTags };  