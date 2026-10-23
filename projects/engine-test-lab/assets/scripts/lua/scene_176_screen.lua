-- Scene 176: the screen. The engine calls on_video_end when a video that
-- does not loop reaches its end.

function on_video_end(entity)
    Event.send("video_ended", entity, { at = "Screen" })
end
